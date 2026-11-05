//! Native turn segments: derived from a chat's messages after every write, so
//! local silver, shared silver and imports all hold the same turns.
use super::generated as sql;
use crate::AppError;
use crate::silver::turns::{self, Boundary, Source};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// What a stored turn records beyond its place in the hierarchy.
#[derive(Serialize, Deserialize)]
struct Metadata {
    revision: String,
    boundary: Boundary,
    closed: bool,
}

/// Bring a chat's turns in line with its current messages.
///
/// Messages are reparented before stale turns are deleted, since deleting a
/// turn deletes whatever is still beneath it.
pub(crate) fn derive(connection: &Connection, chat_id: &str) -> Result<(), AppError> {
    let rows = sql::TurnSources::builder()
        .conversation_id(chat_id)
        .build()
        .query_many(connection)
        .map_err(AppError::database)?;
    let roles = rows
        .iter()
        .map(|row| super::required(row.role.clone(), "role").map(crate::role::Role::from))
        .collect::<Result<Vec<_>, _>>()?;
    let positions = rows
        .iter()
        .map(|row| Ok((row.id.as_str(), super::required(row.ordinal, "ordinal")?)))
        .collect::<Result<HashMap<_, _>, AppError>>()?;
    let sources = rows
        .iter()
        .zip(&roles)
        .map(|(row, role)| {
            Ok(Source {
                id: &row.id,
                ordinal: super::required(row.ordinal, "ordinal")?,
                role,
                projection: row.projection.as_deref().unwrap_or_default(),
                context_parent: match row.context_parent.as_deref() {
                    None => None,
                    Some("") => Some(-1),
                    Some(parent) => Some(
                        *positions
                            .get(parent)
                            .filter(|ordinal| **ordinal < row.ordinal.unwrap_or(-1))
                            .ok_or_else(|| {
                                AppError::database_data("invalid native message parent")
                            })?,
                    ),
                },
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let derived = turns::extract(chat_id, &sources);

    let mut parents = vec![chat_id; rows.len()];
    for turn in &derived {
        let times: Vec<i64> = turn
            .members()
            .filter_map(|index| rows[index].created_at)
            .collect();
        let metadata = serde_json::to_string(&Metadata {
            revision: turn.revision.clone(),
            boundary: turn.boundary,
            closed: turn.closed,
        })
        .map_err(|_| AppError::internal("serialize turn metadata"))?;
        sql::UpsertTurn::builder()
            .id(&turn.id)
            .created_at(times.iter().min().copied())
            .updated_at(times.iter().max().copied())
            .metadata(Some(&metadata))
            .conversation_id(chat_id)
            .build()
            .execute(connection)
            .map_err(AppError::database)?;
        for index in turn.members() {
            parents[index] = &turn.id;
        }
    }
    let storage: HashMap<String, i64> = sql::SegmentStorage::builder()
        .conversation_id(chat_id)
        .build()
        .query_many(connection)
        .map_err(AppError::database)?
        .into_iter()
        .map(|row| (row.id, row.storage_id))
        .collect();
    for (row, parent) in rows.iter().zip(parents) {
        let parent_storage = *storage
            .get(parent)
            .ok_or_else(|| AppError::database_data("native turn parent is missing"))?;
        sql::SetEntryParent::builder()
            .parent_id(Some(parent))
            .parent_storage_id(Some(parent_storage))
            .conversation_id(chat_id)
            .id(&row.id)
            .build()
            .execute(connection)
            .map_err(AppError::database)?;
    }
    let current: BTreeSet<&str> = derived.iter().map(|turn| turn.id.as_str()).collect();
    for stale in sql::ChatTurns::builder()
        .conversation_id(chat_id)
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
pub(crate) fn derive_all(connection: &Connection) -> Result<(), AppError> {
    for chat in sql::NativeChatIds
        .query_many(connection)
        .map_err(AppError::database)?
    {
        derive(connection, &chat.id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
