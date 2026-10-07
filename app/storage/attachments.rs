use super::{Storage, generated as sql};
use crate::{
    AppError,
    attachments::{self, Availability, Listing, Metadata, Retrieved},
};
use std::path::Path;

impl Storage {
    pub(crate) fn attachment_candidates(&self) -> Result<Vec<(String, String)>, AppError> {
        let mut statement = self
            .connection
            .prepare(sql::AttachmentCandidates::QUERY)
            .map_err(AppError::database)?;
        let rows = statement
            .query_map([], sql::AttachmentCandidatesRow::from_row)
            .map_err(AppError::database)?;
        let mut result = Vec::new();
        for row in rows {
            let row = row.map_err(AppError::database)?;
            let records: Vec<Metadata> = serde_json::from_str(&row.attachments)
                .map_err(|_| AppError::database_data("invalid attachments"))?;
            for record in records {
                if record.availability == Availability::Retained {
                    result.push((row.id.clone(), record.id));
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn configure_attachments(
        &mut self,
        config: attachments::Configuration,
    ) -> Result<(), AppError> {
        let bytes = serde_json::to_vec(&config.allowed_roots)
            .map_err(|_| AppError::internal("serialize attachment roots"))?;
        let fingerprint = blake3::hash(&bytes).to_hex().to_string();
        if sql::AttachmentPolicy
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .is_some_and(|row| row.fingerprint != fingerprint)
            || self.cached_attachments_missing()?
        {
            // A root-policy change must replay sources even when history files are
            // unchanged. Keep embeddings when visible dialogue remains identical.
            self.connection.execute_batch("UPDATE source_checkpoints SET interpretation_version=0; UPDATE conversations SET source_fingerprint=''; UPDATE messages SET fingerprint='' WHERE attachments<>'[]';").map_err(AppError::database)?;
        }
        sql::SetAttachmentPolicy::builder()
            .fingerprint(&fingerprint)
            .build()
            .execute(&self.connection)
            .map_err(AppError::database)?;
        self.attachments_config = config;
        Ok(())
    }

    fn cached_attachments_missing(&self) -> Result<bool, AppError> {
        let database = self.connection.path().ok_or_else(|| {
            AppError::internal("attachment storage requires a file-backed database")
        })?;
        let root = attachments::directory(Path::new(database));
        let mut statement = self
            .connection
            .prepare(sql::AttachmentCandidates::QUERY)
            .map_err(AppError::database)?;
        let rows = statement
            .query_map([], sql::AttachmentCandidatesRow::from_row)
            .map_err(AppError::database)?;
        for row in rows {
            let records: Vec<Metadata> =
                serde_json::from_str(&row.map_err(AppError::database)?.attachments)
                    .map_err(|_| AppError::database_data("invalid attachments"))?;
            attachments::validate_metadata(&records)?;
            if records.iter().any(|m| {
                m.availability == Availability::Retained
                    && m.digest
                        .as_deref()
                        .is_some_and(|digest| !root.join(digest).is_file())
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn retain_attachments(
        &self,
        message: &crate::silver::NormalizedMessage,
    ) -> Result<Vec<Metadata>, AppError> {
        let database = self.connection.path().ok_or_else(|| {
            AppError::internal("attachment storage requires a file-backed database")
        })?;
        attachments::retain(
            &attachments::directory(Path::new(database)),
            &self.attachments_config,
            &message.id,
            &message.attachments,
        )
    }

    pub(crate) fn list_attachments(&self, message_id: &str) -> Result<Listing, AppError> {
        let row = sql::MessageAttachments::builder()
            .id(message_id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .ok_or_else(|| AppError::usage("message not found"))?;
        let mut records: Vec<Metadata> = serde_json::from_str(&row.attachments)
            .map_err(|_| AppError::database_data("invalid attachment metadata"))?;
        attachments::validate_metadata(&records)?;
        if let Some(database) = self.connection.path() {
            let root = attachments::directory(Path::new(database));
            for record in &mut records {
                if record.availability == Availability::Retained
                    && record
                        .digest
                        .as_deref()
                        .is_none_or(|digest| !root.join(digest).is_file())
                {
                    record.availability = Availability::Missing;
                }
            }
        }
        Ok(Listing {
            message_id: message_id.into(),
            attachments: records,
        })
    }

    pub(crate) fn get_attachment(&self, message_id: &str, id: &str) -> Result<Retrieved, AppError> {
        let attachment = self
            .list_attachments(message_id)?
            .attachments
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| AppError::usage("attachment not found on this message"))?;
        let database = self.connection.path().ok_or_else(|| {
            AppError::internal("attachment storage requires a file-backed database")
        })?;
        let data = attachments::read(&attachments::directory(Path::new(database)), &attachment)?;
        Ok(Retrieved {
            message_id: message_id.into(),
            attachment,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        provider::{Provider, ProviderRoots},
        silver::{self, IndexOptions},
    };

    #[test]
    fn changing_attachment_roots_replays_unchanged_sources_and_preserves_visible_text() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("session.jsonl");
        let files = root.path().join("files");
        std::fs::create_dir(&files).unwrap();
        let image = files.join("photo.png");
        std::fs::write(&image, [137, 80, 78, 71]).unwrap();
        std::fs::write(&source, serde_json::to_string(&serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn","item":{"type":"UserMessage","id":"message","content":[{"type":"text","text":"real request"},{"type":"local_image","path":image}]}}})).unwrap()).unwrap();
        let options = IndexOptions {
            providers: ProviderRoots::from([(Provider::Codex, vec![root.path().to_path_buf()])]),
            roots_are_authoritative: true,
            since_days: None,
        };
        let db = root.path().join("index.sqlite3");
        let mut writer = Storage::open_writer(&db).unwrap();
        writer
            .configure_attachments(attachments::Configuration::default())
            .unwrap();
        silver::index(&mut writer, &options).unwrap();
        let id = sql::AttachmentCandidates
            .query_many(&writer.connection)
            .unwrap()
            .remove(0)
            .id;
        assert_eq!(
            writer.list_attachments(&id).unwrap().attachments[0].availability,
            Availability::NotAllowed
        );
        writer
            .configure_attachments(attachments::Configuration {
                allowed_roots: vec![files.clone()],
                share_files: false,
            })
            .unwrap();
        silver::index(&mut writer, &options).unwrap();
        assert_eq!(
            writer.list_attachments(&id).unwrap().attachments[0].availability,
            Availability::Retained
        );
        let visible: String = writer
            .connection
            .query_row(
                "SELECT search_projection FROM messages WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(visible, "real request");
        let metadata = writer.list_attachments(&id).unwrap().attachments.remove(0);
        std::fs::remove_file(attachments::directory(&db).join(metadata.digest.as_ref().unwrap()))
            .unwrap();
        assert_eq!(
            writer.list_attachments(&id).unwrap().attachments[0].availability,
            Availability::Missing
        );
        writer
            .configure_attachments(attachments::Configuration {
                allowed_roots: vec![files],
                share_files: false,
            })
            .unwrap();
        silver::index(&mut writer, &options).unwrap();
        assert!(writer.get_attachment(&id, &metadata.id).is_ok());
        writer.commit_writer().unwrap();
    }
}
