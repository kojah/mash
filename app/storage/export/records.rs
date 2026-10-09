//! Temporary read/write boundary while the local SQLite backend is retired.
//! The records it produces are independent of SQLite's schema and row identities.
use super::super::Storage;
use crate::{
    AppError,
    silver::archive::{Record, ScanState},
};
use rusqlite::{Row, params};
use serde::de::DeserializeOwned;

fn get<T: rusqlite::types::FromSql>(row: &Row<'_>, name: &str) -> Result<T, AppError> {
    row.get(name).map_err(AppError::database)
}
fn json<T: DeserializeOwned>(row: &Row<'_>, name: &str) -> Result<T, AppError> {
    serde_json::from_str(&get::<String>(row, name)?)
        .map_err(|_| AppError::database_data("invalid canonical export record"))
}
fn connector_id(row: &Row<'_>, name: &str) -> Result<crate::connectors::Id, AppError> {
    crate::connectors::Id::try_from(get::<String>(row, name)?)
        .map_err(|_| AppError::database_data("invalid connector archive identity"))
}

#[allow(
    clippy::too_many_lines,
    reason = "exhaustive typed canonical table mapping at the temporary SQLite conversion boundary"
)]
pub(crate) fn visit(
    storage: &Storage,
    emit: &mut impl FnMut(Record) -> Result<(), AppError>,
) -> Result<(), AppError> {
    macro_rules! table {
        ($sql:expr, $convert:expr) => {{
            let mut statement = storage
                .connection
                .prepare($sql)
                .map_err(AppError::database)?;
            let mut rows = statement.query([]).map_err(AppError::database)?;
            while let Some(row) = rows.next().map_err(AppError::database)? {
                let record: Record = $convert(row)?;
                record.validate()?;
                emit(record)?;
            }
        }};
    }
    table!("SELECT * FROM conversations ORDER BY id", |row| Ok::<
        _,
        AppError,
    >(
        Record::Conversation {
            id: get(row, "id")?,
            provider: get(row, "provider")?,
            source_path: get(row, "source_path")?,
            source_key: get(row, "source_key")?,
            source_session: get(row, "source_session")?,
            title: get(row, "title")?,
            created_at: get(row, "created_at")?,
            updated_at: get(row, "updated_at")?,
            source_fingerprint: get(row, "source_fingerprint")?,
            project: get(row, "project")?,
            repository: get(row, "repository")?,
        }
    ));
    table!(
        "SELECT * FROM messages ORDER BY conversation_id,ordinal",
        |row| Ok::<_, AppError>(Record::Message {
            attachments: json(row, "attachments")?,
            id: get(row, "id")?,
            conversation_id: get(row, "conversation_id")?,
            ordinal: get(row, "ordinal")?,
            role: get(row, "role")?,
            content: get(row, "content")?,
            dialogue_text: get(row, "search_projection")?,
            context: crate::message_context::MessageContext::from_parent(get(
                row,
                "context_parent"
            )?)?,
            created_at: get(row, "created_at")?,
            fingerprint: get(row, "fingerprint")?,
        })
    );
    table!(
        "SELECT * FROM conversation_enrichment ORDER BY conversation_id",
        |row| Ok::<_, AppError>(Record::Enrichment {
            conversation_id: get(row, "conversation_id")?,
            evidence: json(row, "evidence")?,
        })
    );
    table!(
        "SELECT * FROM tombstones ORDER BY provider,conversation_id",
        |row| Ok::<_, AppError>(Record::Tombstone {
            provider: get(row, "provider")?,
            conversation_id: get(row, "conversation_id")?,
            forgotten_at: get(row, "forgotten_at")?,
        })
    );
    table!(
        "SELECT * FROM claude_sources ORDER BY source_key",
        |row| Ok::<_, AppError>(Record::ClaudeSource {
            source_key: get(row, "source_key")?,
            source_path: get(row, "source_path")?,
            conversation_id: get(row, "conversation_id")?,
            title: get(row, "title")?,
            project: get(row, "project")?,
            repository: get(row, "repository")?,
        })
    );
    table!(
        "SELECT * FROM claude_source_messages ORDER BY source_key,message_id",
        |row| Ok::<_, AppError>(Record::ClaudeMessage {
            attachments: retained_attachments(row)?,
            source_key: get(row, "source_key")?,
            message_id: get(row, "message_id")?,
            parent_id: get(row, "parent_id")?,
            role: get(row, "role")?,
            content: get(row, "content")?,
            dialogue_text: get(row, "projection")?,
            created_at: get(row, "created_at")?,
        })
    );
    table!(
        "SELECT * FROM source_checkpoints ORDER BY provider,source_key",
        |row| Ok::<_, AppError>(Record::Checkpoint {
            provider: get(row, "provider")?,
            source_path: get(row, "source_path")?,
            source_key: get(row, "source_key")?,
            size_bytes: get(row, "size_bytes")?,
            modified_ns: get(row, "modified_ns")?,
            fingerprint: get(row, "fingerprint")?,
            interpretation_version: get(row, "interpretation_version")?,
        })
    );
    table!("SELECT * FROM silver_scans ORDER BY provider", |row| Ok::<
        _,
        AppError,
    >(
        Record::Scan {
            provider: get(row, "provider")?,
            interpretation_version: get(row, "interpretation_version")?,
            state: match get::<i64>(row, "state")? {
                0 => ScanState::Pending,
                1 => ScanState::Complete,
                2 => ScanState::Incomplete,
                _ => return Err(AppError::database_data("invalid canonical scan state")),
            },
        }
    ));
    table!(
        "SELECT * FROM silver_rejections ORDER BY provider,source_key",
        |row| Ok::<_, AppError>(Record::Rejection {
            provider: get(row, "provider")?,
            source_key: get(row, "source_key")?,
            source_path: get(row, "source_path")?,
            modified_ns: get(row, "modified_ns")?,
        })
    );
    table!(
        "SELECT fingerprint FROM attachment_policy WHERE singleton=1",
        |row| Ok::<_, AppError>(Record::AttachmentPolicy {
            fingerprint: get(row, "fingerprint")?,
        })
    );
    table!(
        "SELECT instance,revision FROM shared_publication WHERE singleton=1",
        |row| Ok::<_, AppError>(Record::Publication {
            instance: get(row, "instance")?,
            revision: get(row, "revision")?,
        })
    );
    table!(
        "SELECT id,revision FROM connector_sources ORDER BY id",
        |row| Ok::<_, AppError>(Record::ConnectorSource {
            source_id: connector_id(row, "id")?,
            revision: get::<String>(row, "revision")?
                .parse()
                .map_err(|_| AppError::database_data("invalid connector source revision"))?,
        })
    );
    table!(
        "SELECT source_id,schema FROM connector_schemas ORDER BY source_id",
        |row| Ok::<_, AppError>(Record::ConnectorSchema {
            source_id: connector_id(row, "source_id")?,
            schema: json(row, "schema")?,
        })
    );
    table!(
        "SELECT source_id,record FROM connector_objects ORDER BY source_id,id",
        |row| Ok::<_, AppError>(Record::ConnectorObject {
            source_id: connector_id(row, "source_id")?,
            object: json(row, "record")?,
        })
    );
    table!(
        "SELECT fingerprint,receipt FROM connector_receipts ORDER BY source_id,batch_id",
        |row| Ok::<_, AppError>(Record::ConnectorReceipt {
            fingerprint: get(row, "fingerprint")?,
            receipt: json(row, "receipt")?,
        })
    );
    table!(
        "SELECT revision,interpretation_version FROM silver_state WHERE singleton=1",
        |row| Ok::<_, AppError>(Record::State {
            revision: u64::try_from(get::<i64>(row, "revision")?)
                .map_err(|_| AppError::database_data("invalid canonical revision"))?,
            interpretation_version: get(row, "interpretation_version")?,
        })
    );
    Ok(())
}

fn retained_attachments(row: &Row<'_>) -> Result<Vec<crate::attachments::Metadata>, AppError> {
    let sources: Vec<crate::attachments::Source> = json(row, "attachments")?;
    sources
        .into_iter()
        .map(|source| match source {
            crate::attachments::Source::Retained { metadata } => Ok(metadata),
            _ => Err(AppError::database_data(
                "invalid export attachment metadata",
            )),
        })
        .collect()
}

#[allow(
    clippy::too_many_lines,
    reason = "exhaustive typed record mapping for the portable silver format"
)]
pub(crate) fn insert(storage: &Storage, record: Record) -> Result<(), AppError> {
    record.validate()?;
    let connection = &storage.connection;
    macro_rules! execute {
        ($sql:expr, $($value:expr),* $(,)?) => {
            connection.execute($sql, params![$($value),*]).map_err(AppError::database)?
        };
    }
    match record {
        Record::ConnectorSource {
            source_id,
            revision,
        } => {
            execute!(
                "INSERT INTO connector_sources(id,revision) VALUES(?1,?2)",
                source_id.as_str(),
                revision.to_string()
            );
        }
        Record::ConnectorSchema { source_id, schema } => {
            execute!(
                "INSERT INTO connector_schemas(source_id,schema) VALUES(?1,?2)",
                source_id.as_str(),
                encode(&schema)?
            );
        }
        Record::ConnectorObject { source_id, object } => {
            // The parent edge is deferred, so objects may precede their parents.
            execute!(
                "INSERT INTO connector_objects(source_id,id,role,parent_id,reply_to,record) VALUES(?1,?2,?3,?4,?5,?6)",
                source_id.as_str(),
                object.id.as_str(),
                crate::connectors::silver::role_name(object.role),
                object.parent.as_ref().map(crate::connectors::Id::as_str),
                object.reply_to.as_ref().map(crate::connectors::Id::as_str),
                encode(&object)?
            );
        }
        Record::ConnectorReceipt {
            fingerprint,
            receipt,
        } => {
            execute!(
                "INSERT INTO connector_receipts(source_id,batch_id,fingerprint,receipt) VALUES(?1,?2,?3,?4)",
                receipt.source_id.as_str(),
                receipt.batch_id.as_str(),
                fingerprint,
                encode(&receipt)?
            );
        }
        Record::Publication { instance, revision } => {
            if instance.len() != 32
                || !instance.bytes().all(|b| b.is_ascii_hexdigit())
                || revision < 1
            {
                return Err(AppError::usage("invalid publication identity"));
            }
            execute!(
                "UPDATE shared_publication SET instance=?1,revision=?2 WHERE singleton=1",
                instance,
                revision
            );
        }
        Record::State {
            revision,
            interpretation_version,
        } => {
            execute!(
                "UPDATE silver_state SET revision=?1,interpretation_version=?2 WHERE singleton=1",
                i64::try_from(revision).map_err(|_| AppError::usage(
                    "canonical revision exceeds local storage capacity"
                ))?,
                interpretation_version
            );
        }
        Record::Conversation {
            id,
            provider,
            source_path,
            source_key,
            source_session,
            title,
            created_at,
            updated_at,
            source_fingerprint,
            project,
            repository,
        } => {
            execute!(
                "INSERT INTO conversations(id,provider,source_path,source_key,source_session,title,created_at,updated_at,source_fingerprint,project,repository) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                id,
                provider,
                source_path,
                source_key,
                source_session,
                title,
                created_at,
                updated_at,
                source_fingerprint,
                project,
                repository
            );
        }
        Record::Message {
            attachments,
            id,
            conversation_id,
            ordinal,
            role,
            content,
            dialogue_text,
            context,
            created_at,
            fingerprint,
        } => {
            execute!(
                "INSERT INTO messages(attachments,id,conversation_id,ordinal,role,content,search_projection,context_parent,created_at,fingerprint) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                encode(&attachments)?,
                id,
                conversation_id,
                ordinal,
                role,
                content,
                dialogue_text,
                context.parent(),
                created_at,
                fingerprint
            );
        }
        Record::Enrichment {
            conversation_id,
            evidence,
        } => {
            execute!(
                "INSERT INTO conversation_enrichment(conversation_id,evidence) VALUES(?1,?2)",
                conversation_id,
                encode(&evidence)?
            );
        }
        Record::Tombstone {
            provider,
            conversation_id,
            forgotten_at,
        } => {
            execute!(
                "INSERT INTO tombstones(provider,conversation_id,forgotten_at) VALUES(?1,?2,?3)",
                provider,
                conversation_id,
                forgotten_at
            );
        }
        Record::ClaudeSource {
            source_key,
            source_path,
            conversation_id,
            title,
            project,
            repository,
        } => {
            execute!(
                "INSERT INTO claude_sources(source_key,source_path,conversation_id,title,project,repository) VALUES(?1,?2,?3,?4,?5,?6)",
                source_key,
                source_path,
                conversation_id,
                title,
                project,
                repository
            );
        }
        Record::ClaudeMessage {
            attachments,
            source_key,
            message_id,
            parent_id,
            role,
            content,
            dialogue_text,
            created_at,
        } => {
            let sources = attachments
                .into_iter()
                .map(|metadata| crate::attachments::Source::Retained { metadata })
                .collect::<Vec<_>>();
            execute!(
                "INSERT INTO claude_source_messages(attachments,source_key,message_id,parent_id,role,content,projection,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                encode(&sources)?,
                source_key,
                message_id,
                parent_id,
                role,
                content,
                dialogue_text,
                created_at
            );
        }
        Record::Checkpoint {
            provider,
            source_path,
            source_key,
            size_bytes,
            modified_ns,
            fingerprint,
            interpretation_version,
        } => {
            execute!(
                "INSERT INTO source_checkpoints(provider,source_path,source_key,size_bytes,modified_ns,fingerprint,interpretation_version) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                provider,
                source_path,
                source_key,
                size_bytes,
                modified_ns,
                fingerprint,
                interpretation_version
            );
        }
        Record::Scan {
            provider,
            interpretation_version,
            state,
        } => {
            let state = match state {
                ScanState::Pending => 0,
                ScanState::Complete => 1,
                ScanState::Incomplete => 2,
            };
            execute!(
                "INSERT INTO silver_scans(provider,interpretation_version,state) VALUES(?1,?2,?3)",
                provider,
                interpretation_version,
                state
            );
        }
        Record::Rejection {
            provider,
            source_key,
            source_path,
            modified_ns,
        } => {
            execute!(
                "INSERT INTO silver_rejections(provider,source_key,source_path,modified_ns) VALUES(?1,?2,?3,?4)",
                provider,
                source_key,
                source_path,
                modified_ns
            );
        }
        Record::AttachmentPolicy { fingerprint } => {
            execute!(
                "INSERT INTO attachment_policy(singleton,fingerprint) VALUES(1,?1)",
                fingerprint
            );
        }
    }
    Ok(())
}

fn encode(value: &impl serde::Serialize) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map_err(|_| AppError::internal("serialize canonical export record"))
}
