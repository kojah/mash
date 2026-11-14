//! Read only the dependencies of search projection from the common silver schema.
use crate::{AppError, views::documents::SearchMessage};
use rusqlite::Connection;

pub fn visit(
    connection: &Connection,
    chat: Option<&str>,
    mut accept: impl FnMut(SearchMessage) -> Result<(), AppError>,
) -> Result<(), AppError> {
    // COALESCE short-circuits: explicit projections, including empty ones, do
    // not decode canonical content. Keep lightweight hidden rows to preserve
    // parent lookup and the materializer's inference batch boundaries.
    let suffix = if chat.is_some() {
        "WHERE chat_id=?1 ORDER BY ordinal"
    } else {
        "ORDER BY chat_id, ordinal"
    };
    let sql = format!(
        "SELECT id,chat_id,ordinal,role,COALESCE(search_projection,content),
         created_at,context_parent FROM messages {suffix}"
    );
    let mut statement = connection.prepare(&sql).map_err(AppError::database)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(chat))
        .map_err(AppError::database)?;
    while let Some(row) = rows.next().map_err(AppError::database)? {
        accept(SearchMessage {
            id: row.get(0).map_err(AppError::database)?,
            chat_id: row.get(1).map_err(AppError::database)?,
            ordinal: row.get(2).map_err(AppError::database)?,
            role: row.get(3).map_err(AppError::database)?,
            projection: row.get(4).map_err(AppError::database)?,
            created_at: row.get(5).map_err(AppError::database)?,
            context: crate::message_context::MessageContext::from_parent(
                row.get(6).map_err(AppError::database)?,
            ),
            generation: None,
            vector: None,
        })?;
    }
    Ok(())
}

pub fn chat_messages(connection: &Connection, chat: &str) -> Result<Vec<SearchMessage>, AppError> {
    let mut messages = Vec::new();
    visit(connection, Some(chat), |message| {
        messages.push(message);
        Ok(())
    })?;
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rusqlite::{functions::FunctionFlags, types::ValueRef};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::storage::schema::initialize_silver(&mut connection).unwrap();
        connection
            .execute("INSERT INTO chats(id,provider) VALUES ('chat','codex')", [])
            .unwrap();
        connection
    }

    #[test]
    fn search_input_decodes_only_canonical_fallback_and_ignores_tool_payloads() {
        let connection = database();
        for (ordinal, projection) in [Some(""), Some("visible"), None].into_iter().enumerate() {
            connection.execute(
                "INSERT INTO messages(id,chat_id,ordinal,role,content_packed,search_projection,metadata,attachments)
                 VALUES (?1,'chat',?2,'user',silver_pack('canonical fallback'),?3,'unused opaque tools','unused attachments')",
                rusqlite::params![format!("m{ordinal}"), i64::try_from(ordinal).unwrap(), projection],
            ).unwrap();
        }
        let decodes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&decodes);
        connection
            .create_scalar_function(
                "silver_text",
                1,
                FunctionFlags::SQLITE_UTF8
                    | FunctionFlags::SQLITE_DETERMINISTIC
                    | FunctionFlags::SQLITE_INNOCUOUS,
                move |ctx| {
                    counter.fetch_add(1, Ordering::Relaxed);
                    Ok(match ctx.get_raw(0) {
                        ValueRef::Text(text) => std::str::from_utf8(text).unwrap().to_owned(),
                        ValueRef::Blob(frame) => crate::storage::packed::unpack(frame).unwrap(),
                        _ => panic!("fixture content type"),
                    })
                },
            )
            .unwrap();
        let messages = chat_messages(&connection, "chat").unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|m| m.projection.as_str())
                .collect::<Vec<_>>(),
            ["", "visible", "canonical fallback"]
        );
        assert_eq!(decodes.load(Ordering::Relaxed), 1);
        assert!(chat_messages(&connection, "absent").unwrap().is_empty());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        #[test]
        fn search_input_preserves_hidden_positions_and_parent_links(
            rows in prop::collection::vec((any::<bool>(), "[a-z]{1,20}"), 1..160),
        ) {
            let connection = database();
            for (ordinal, (visible, text)) in rows.iter().enumerate() {
                let parent = ordinal.checked_sub(1).map(|i| format!("m{i}"));
                connection.execute(
                    "INSERT INTO messages(id,chat_id,ordinal,role,content_packed,search_projection,context_parent)
                     VALUES (?1,'chat',?2,'user',silver_pack('canonical'),?3,?4)",
                    rusqlite::params![format!("m{ordinal}"), i64::try_from(ordinal).unwrap(), if *visible {text.as_str()} else {""}, parent],
                ).unwrap();
            }
            let mut captured = Vec::new();
            visit(&connection, None, |message| { captured.push(message); Ok(()) }).unwrap();
            prop_assert_eq!(captured.len(), rows.len());
            for (ordinal, (message, (visible, text))) in captured.iter().zip(&rows).enumerate() {
                prop_assert_eq!(message.ordinal, i64::try_from(ordinal).unwrap());
                prop_assert_eq!(&message.id, &format!("m{ordinal}"));
                prop_assert_eq!(message.projection.as_str(), if *visible { text.as_str() } else { "" });
                let expected_parent = ordinal.checked_sub(1).map(|i| format!("m{i}"));
                prop_assert_eq!(message.context.parent(), expected_parent.as_deref());
            }
        }
    }
}
