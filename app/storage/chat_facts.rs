//! Facts about a chat that search results show: its title, times and
//! enrichment. They live once with the chat in silver, never on a message's
//! search document, so a new message leaves every other document untouched.
use crate::AppError;
use crate::storage::SearchHit;
use rusqlite::Connection;
use std::collections::HashMap;

#[derive(Clone)]
pub(crate) struct Facts {
    title: Option<String>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
    enrichment: Option<crate::silver::enrichment::Evidence>,
}

/// Read a chat's facts while checking its canonical provider. A document can
/// briefly outlive a deleted chat, in which case it has none.
pub(crate) fn read(
    connection: &Connection,
    provider: crate::provider::Provider,
    chat_id: &str,
) -> Result<Option<Facts>, AppError> {
    use rusqlite::OptionalExtension;
    let row = connection
        .prepare_cached(
            "SELECT c.provider, c.title, c.created_at, c.updated_at, e.evidence
               FROM chats c LEFT JOIN chat_enrichment e ON e.chat_id = c.id
              WHERE c.id = ?1",
        )
        .map_err(AppError::database)?
        .query_row([chat_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .optional()
        .map_err(AppError::database)?;
    let Some((source, title, created_at, updated_at, evidence)) = row else {
        return Ok(None);
    };
    if source != provider.as_str() {
        return Err(AppError::database_data(
            "search chat identity disagrees with silver",
        ));
    }
    Ok(Some(Facts {
        title,
        created_at,
        updated_at,
        enrichment: evidence
            .map(|json| super::enrichment::decode(&json))
            .transpose()?,
    }))
}

impl Facts {
    pub(crate) fn apply(self, hit: &mut SearchHit) {
        hit.metadata.title = self.title;
        hit.metadata.created_at = self.created_at;
        hit.metadata.updated_at = self.updated_at;
        hit.metadata.enrichment = self.enrichment;
    }
}

impl super::Storage {
    pub(crate) fn hydrate_chats(
        &self,
        candidates: &mut crate::views::retrieval::Candidates,
    ) -> Result<(), AppError> {
        let mut chats = HashMap::new();
        for hit in candidates
            .lexical
            .iter_mut()
            .chain(&mut candidates.semantic)
        {
            let key = (hit.provider, hit.chat_id.clone());
            if !chats.contains_key(&key) {
                let facts = read(&self.connection, hit.provider, &hit.chat_id)?;
                chats.insert(key.clone(), facts);
            }
            if let Some(facts) = chats.get(&key).cloned().flatten() {
                facts.apply(hit);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::Storage;
    use crate::storage::tests::chat_with_messages;

    #[test]
    fn search_hits_take_chat_facts_from_silver_and_follow_later_changes() {
        let directory = tempfile::tempdir().unwrap();
        let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
        let mut chat = chat_with_messages(&[("ask", "question")]);
        chat.title = Some("First title".into());
        chat.created_at = Some(100);
        chat.updated_at = Some(200);
        storage.replace_chat(&chat).unwrap();
        let read = |storage: &Storage| {
            super::read(&storage.connection, chat.provider, &chat.id)
                .unwrap()
                .unwrap()
        };
        let facts = read(&storage);
        assert_eq!(facts.title.as_deref(), Some("First title"));
        assert_eq!((facts.created_at, facts.updated_at), (Some(100), Some(200)));

        // A new message moves the chat's time; no search document says so.
        chat.title = Some("Second title".into());
        chat.updated_at = Some(300);
        storage.replace_chat(&chat).unwrap();
        let facts = read(&storage);
        assert_eq!(facts.title.as_deref(), Some("Second title"));
        assert_eq!(facts.updated_at, Some(300));

        assert!(
            super::read(&storage.connection, chat.provider, "deleted")
                .unwrap()
                .is_none()
        );
        let other = crate::provider::Provider::ALL
            .iter()
            .copied()
            .find(|provider| *provider != chat.provider)
            .unwrap();
        assert!(super::read(&storage.connection, other, &chat.id).is_err());
    }
}
