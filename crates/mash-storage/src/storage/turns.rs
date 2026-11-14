//! Agent turns: derived from a chat's messages after every write, so
//! local silver, shared silver and imports all hold the same turns.
use super::generated as sql;
use crate::AppError;
use crate::silver::turns::{self, Boundary, Source};
use rusqlite::Connection;
use std::collections::{BTreeSet, HashMap};

/// Resolve a message's derived turn while checking its canonical chat identity.
/// A point can briefly outlive a deleted silver message, in which case it has
/// no turn reference. Turns themselves are never stored in Qdrant.
pub fn message_turn(
    connection: &Connection,
    provider: crate::provider::Provider,
    chat_id: &str,
    message_id: &str,
) -> Result<Option<String>, AppError> {
    use rusqlite::OptionalExtension;
    let row = connection
        .prepare_cached(
            "SELECT c.provider, m.chat_id, m.turn_id, t.chat_id
               FROM messages m JOIN chats c ON c.id=m.chat_id
               LEFT JOIN turns t ON t.id = m.turn_id
              WHERE m.id = ?1",
        )
        .map_err(AppError::database)?
        .query_row([message_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .optional()
        .map_err(AppError::database)?;
    let Some((source, chat, turn, turn_chat)) = row else {
        return Ok(None);
    };
    if source != provider.as_str() || chat != chat_id {
        return Err(AppError::database_data(
            "search message identity disagrees with silver",
        ));
    }
    if turn.is_some() && turn_chat.as_deref() != Some(chat_id) {
        return Err(AppError::database_data(
            "message turn belongs to another chat",
        ));
    }
    Ok(turn)
}

impl super::Storage {
    pub fn hydrate_turns(
        &self,
        candidates: &mut crate::views::candidates::Candidates,
    ) -> Result<(), AppError> {
        for hit in candidates
            .lexical
            .iter_mut()
            .chain(&mut candidates.semantic)
        {
            hit.turn_id = message_turn(&self.connection, hit.provider, &hit.chat_id, &hit.id)?;
        }
        Ok(())
    }
}

/// Bring a chat's derived turns and message references up to date.
/// Deleting stale turns cannot delete canonical messages.
pub fn derive(connection: &Connection, chat_id: &str) -> Result<(), AppError> {
    let rows = sql::TurnSources::builder()
        .chat_id(chat_id)
        .build()
        .query_many(connection)
        .map_err(AppError::database)?;
    let roles = rows
        .iter()
        .map(|row| crate::role::Role::from(row.role.clone()))
        .collect::<Vec<_>>();
    let positions = rows
        .iter()
        .map(|row| (row.id.as_str(), row.ordinal))
        .collect::<HashMap<_, _>>();
    let sources = rows
        .iter()
        .zip(&roles)
        .map(|(row, role)| {
            Ok(Source {
                id: &row.id,
                ordinal: row.ordinal,
                role,
                projection: &row.projection,
                context_parent: match row.context_parent.as_deref() {
                    None => None,
                    Some("") => Some(-1),
                    Some(parent) => Some(
                        *positions
                            .get(parent)
                            .filter(|ordinal| **ordinal < row.ordinal)
                            .ok_or_else(|| {
                                AppError::database_data("invalid native message parent")
                            })?,
                    ),
                },
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let derived = turns::extract(chat_id, &sources);

    let mut membership = vec![None; rows.len()];
    for turn in &derived {
        let times: Vec<i64> = turn
            .members()
            .filter_map(|index| rows[index].created_at)
            .collect();
        let boundary = match turn.boundary {
            Boundary::Start => "start",
            Boundary::UserReply => "user_reply",
            Boundary::Branch => "branch",
            Boundary::Orphan => "orphan",
        };
        sql::UpsertTurn::builder()
            .id(&turn.id)
            .created_at(times.iter().min().copied())
            .updated_at(times.iter().max().copied())
            .revision(&turn.revision)
            .boundary(boundary)
            .closed(i64::from(turn.closed))
            .chat_id(chat_id)
            .build()
            .execute(connection)
            .map_err(AppError::database)?;
        for index in turn.members() {
            membership[index] = Some(turn.id.as_str());
        }
    }
    for (row, turn_id) in rows.iter().zip(membership) {
        sql::SetMessageTurn::builder()
            .turn_id(turn_id)
            .chat_id(chat_id)
            .id(&row.id)
            .build()
            .execute(connection)
            .map_err(AppError::database)?;
    }
    let current: BTreeSet<&str> = derived.iter().map(|turn| turn.id.as_str()).collect();
    for stale in sql::ChatTurns::builder()
        .chat_id(chat_id)
        .build()
        .query_many(connection)
        .map_err(AppError::database)?
    {
        if !current.contains(stale.id.as_str()) {
            sql::DeleteTurn::builder()
                .id(&stale.id)
                .build()
                .execute(connection)
                .map_err(AppError::database)?;
        }
    }
    Ok(())
}

/// Derive every chat's turns, after an import has restored their messages.
pub fn derive_all(connection: &Connection) -> Result<(), AppError> {
    for chat in sql::ChatIds
        .query_many(connection)
        .map_err(AppError::database)?
    {
        derive(connection, &chat.id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
