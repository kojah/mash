//! Gold projection of connector silver: searchable messages and documents.
//!
//! Connector records have no native message rowid, provider or role. Each unit
//! is keyed by a namespaced anchor, indexed for lexical search, and embedded
//! once per generation. Units are synchronized from silver incrementally by
//! content digest, so unchanged records keep their FTS rows and vectors.
use super::generated as sql;
use super::{SearchHit, Storage};
use crate::AppError;
use crate::connectors::{AuthorKind, Chat, Document, Message};
use crate::source::{Kind, Source};
use rusqlite::params;
use std::collections::HashMap;

/// Every connector anchor starts with this; native message IDs never do.
pub(crate) const ANCHOR_PREFIX: &str = "connector#";

/// `connector#<kind>#<source_id>#<id>`. Connector IDs exclude `#`, so the
/// anchor splits unambiguously.
pub(crate) fn anchor(kind: &str, source_id: &str, id: &str) -> String {
    format!("{ANCHOR_PREFIX}{kind}#{source_id}#{id}")
}

/// The kind, source and record ID named by an anchor.
pub(crate) fn parse_anchor(anchor: &str) -> Option<(&str, &str, &str)> {
    let mut parts = anchor.strip_prefix(ANCHOR_PREFIX)?.splitn(3, '#');
    Some((parts.next()?, parts.next()?, parts.next()?))
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Message => "message",
        Kind::Document => "document",
    }
}

/// The text indexed and embedded for a unit. A chat or document title gives a
/// short message context, much as native embeddings prefix the chat title.
fn unit_text(title: Option<&str>, content: &str) -> String {
    match title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => format!("{title}\n{content}"),
        None => content.to_owned(),
    }
}

struct Unit {
    anchor: String,
    kind: Kind,
    source_id: String,
    record_id: String,
    chat_anchor: String,
    created_at: Option<i64>,
    text: String,
}

fn decode<T: serde::de::DeserializeOwned>(record: &str) -> Result<T, AppError> {
    serde_json::from_str(record)
        .map_err(|_| AppError::database_data("stored connector record is invalid"))
}

/// A connector unit's evidence, hydrated from silver.
struct Evidence {
    content: String,
    title: Option<String>,
    author: Option<AuthorKind>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
    chat_created_at: Option<i64>,
    chat_updated_at: Option<i64>,
}

impl Storage {
    /// Bring connector units, their FTS rows and vector invalidation in line
    /// with connector silver. Runs inside the gold build's writer.
    pub(crate) fn sync_connector_units(&mut self) -> Result<(), AppError> {
        self.require_writer()?;
        let mut known = sql::ConnectorUnitDigests
            .query_many(&self.connection)
            .map_err(AppError::database)?
            .into_iter()
            .map(|row| (row.anchor, (row.id, row.digest)))
            .collect::<HashMap<_, _>>();
        for unit in self.connector_silver_units()? {
            let digest = blake3::hash(unit.text.as_bytes()).as_bytes().to_vec();
            match known.remove(&unit.anchor) {
                Some((_, previous)) if previous == digest => {}
                Some((id, _)) => {
                    sql::ConnectorUpdateUnit::builder()
                        .chat_anchor(&unit.chat_anchor)
                        .created_at(unit.created_at)
                        .digest(&digest)
                        .id(id)
                        .build()
                        .execute(&self.connection)
                        .map_err(AppError::database)?;
                    self.delete_connector_text(id)?;
                    self.insert_connector_text(id, &unit.text)?;
                    sql::ConnectorDeleteUnitEmbedding::builder()
                        .unit_id(id)
                        .build()
                        .execute(&self.connection)
                        .map_err(AppError::database)?;
                }
                None => {
                    let id = sql::ConnectorInsertUnit::builder()
                        .anchor(&unit.anchor)
                        .kind(kind_name(unit.kind))
                        .source_id(&unit.source_id)
                        .record_id(&unit.record_id)
                        .chat_anchor(&unit.chat_anchor)
                        .created_at(unit.created_at)
                        .digest(&digest)
                        .build()
                        .query_one(&self.connection)
                        .map_err(AppError::database)?
                        .id;
                    self.insert_connector_text(id, &unit.text)?;
                }
            }
        }
        for (_, (id, _)) in known {
            self.delete_connector_text(id)?;
            sql::ConnectorDeleteUnitEmbedding::builder()
                .unit_id(id)
                .build()
                .execute(&self.connection)
                .map_err(AppError::database)?;
            sql::ConnectorDeleteUnit::builder()
                .id(id)
                .build()
                .execute(&self.connection)
                .map_err(AppError::database)?;
        }
        Ok(())
    }

    // sqlc cannot analyze FTS5 virtual tables; these statements stay handwritten.
    fn insert_connector_text(&self, id: i64, text: &str) -> Result<(), AppError> {
        self.connection
            .execute(
                "INSERT INTO connector_fts(rowid, content) VALUES (?1, ?2)",
                params![id, text],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    fn delete_connector_text(&self, id: i64) -> Result<(), AppError> {
        self.connection
            .execute("DELETE FROM connector_fts WHERE rowid = ?1", [id])
            .map_err(AppError::database)?;
        Ok(())
    }

    fn connector_silver_units(&self) -> Result<Vec<Unit>, AppError> {
        let mut units = Vec::new();
        for row in sql::ConnectorSilverMessages
            .query_many(&self.connection)
            .map_err(AppError::database)?
        {
            let message: Message = decode(&row.message)?;
            let chat: Chat = decode(&row.chat)?;
            if message.content.trim().is_empty() {
                continue;
            }
            units.push(Unit {
                anchor: anchor("message", &row.source_id, message.id.as_str()),
                kind: Kind::Message,
                chat_anchor: anchor("chat", &row.source_id, message.chat_id.as_str()),
                record_id: message.id.as_str().to_owned(),
                created_at: message.created_at,
                text: unit_text(chat.title.as_deref(), &message.content),
                source_id: row.source_id,
            });
        }
        for row in sql::ConnectorSilverDocuments
            .query_many(&self.connection)
            .map_err(AppError::database)?
        {
            let document: Document = decode(&row.record)?;
            if document.content.trim().is_empty() {
                continue;
            }
            let own = anchor("document", &row.source_id, document.id.as_str());
            units.push(Unit {
                chat_anchor: own.clone(),
                anchor: own,
                kind: Kind::Document,
                record_id: document.id.as_str().to_owned(),
                created_at: document.created_at,
                text: unit_text(document.title.as_deref(), &document.content),
                source_id: row.source_id,
            });
        }
        Ok(units)
    }

    /// Ranked unit IDs and BM25 scores for an FTS5 match expression. Handwritten:
    /// sqlc cannot analyze FTS5, and the filter predicate is compiled at runtime.
    pub(crate) fn search_connector_lexical(
        &self,
        query: &str,
        limit: usize,
        cutoff: Option<i64>,
        filters: Option<&crate::search_filter::Scope>,
    ) -> Result<Vec<(i64, f64)>, AppError> {
        let predicate = filters.map_or_else(
            || "TRUE".to_owned(),
            |f| f.filter.connector_sql(Some(&f.node)),
        );
        let limit = i64::try_from(limit.min(1_000)).unwrap_or(1_000);
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT u.id, bm25(connector_fts) AS score
                   FROM connector_fts
                   JOIN connector_units u ON u.id = connector_fts.rowid
                  WHERE connector_fts MATCH ?1
                    AND (?2 IS NULL OR u.created_at >= ?2)
                    AND ({predicate})
                  ORDER BY score, u.id
                  LIMIT ?3"
            ))
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(params![query, cutoff, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(AppError::database)?;
        rows.collect::<Result<_, _>>().map_err(AppError::database)
    }

    /// Current-generation vectors of units the filters admit. Handwritten for the
    /// runtime-compiled filter predicate.
    pub(crate) fn connector_vectors(
        &self,
        generation: &str,
        cutoff: Option<i64>,
        filters: Option<&crate::search_filter::Scope>,
    ) -> Result<Vec<(i64, f32, Vec<u8>)>, AppError> {
        let predicate = filters.map_or_else(
            || "TRUE".to_owned(),
            |f| f.filter.connector_sql(Some(&f.node)),
        );
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT u.id, e.norm, e.vector
                   FROM connector_embeddings e
                   JOIN connector_units u ON u.id = e.unit_id
                  WHERE e.generation = ?1
                    AND (?2 IS NULL OR u.created_at >= ?2)
                    AND ({predicate})"
            ))
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(params![generation, cutoff], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(AppError::database)?;
        rows.map(|row| {
            let (id, norm, vector) = row.map_err(AppError::database)?;
            #[allow(clippy::cast_possible_truncation)]
            Ok((id, norm as f32, vector))
        })
        .collect()
    }

    /// Search hits for unit IDs, in the given order.
    pub(crate) fn connector_hits(&self, ids: &[i64]) -> Result<Vec<SearchHit>, AppError> {
        ids.iter()
            .map(|id| {
                let unit = sql::ConnectorUnit::builder()
                    .id(*id)
                    .build()
                    .query_opt(&self.connection)
                    .map_err(AppError::database)?
                    .ok_or_else(|| {
                        AppError::database_data("connector index references a missing unit")
                    })?;
                let kind = unit_kind(&unit.kind);
                let evidence = self.connector_evidence(kind, &unit.source_id, &unit.record_id)?;
                Ok(SearchHit {
                    lookup: None,
                    lexical_match: None,
                    id: unit.anchor,
                    conversation_id: unit.chat_anchor,
                    source: Source::Connector(unit.source_id),
                    kind,
                    role: None,
                    author_kind: evidence.author,
                    content: evidence.content,
                    created_at: evidence.created_at,
                    metadata: crate::search_results::ChatMetadata {
                        enrichment: None,
                        title: evidence.title,
                        project: None,
                        repository: None,
                        created_at: evidence.chat_created_at,
                        updated_at: evidence.chat_updated_at.or(evidence.updated_at),
                    },
                    copies: Vec::new(),
                    lexical_score: None,
                    semantic_score: None,
                    fusion_score: 0.0,
                    rerank_score: None,
                    node_id: None,
                })
            })
            .collect()
    }

    fn connector_evidence(
        &self,
        kind: Kind,
        source_id: &str,
        record_id: &str,
    ) -> Result<Evidence, AppError> {
        let missing = || AppError::database_data("connector index references missing silver");
        match kind {
            Kind::Message => {
                let row = sql::ConnectorMessageEvidence::builder()
                    .source_id(source_id)
                    .id(record_id)
                    .build()
                    .query_opt(&self.connection)
                    .map_err(AppError::database)?
                    .ok_or_else(missing)?;
                let message: Message = decode(&row.message)?;
                let chat: Chat = decode(&row.chat)?;
                Ok(Evidence {
                    content: message.content,
                    title: chat.title,
                    author: Some(
                        message
                            .author
                            .map_or(AuthorKind::Unknown, |author| author.kind),
                    ),
                    created_at: message.created_at,
                    updated_at: message.updated_at,
                    chat_created_at: chat.created_at,
                    chat_updated_at: chat.updated_at,
                })
            }
            Kind::Document => {
                let row = sql::ConnectorDocumentEvidence::builder()
                    .source_id(source_id)
                    .id(record_id)
                    .build()
                    .query_opt(&self.connection)
                    .map_err(AppError::database)?
                    .ok_or_else(missing)?;
                let document: Document = decode(&row.record)?;
                Ok(Evidence {
                    content: document.content,
                    title: document.title,
                    author: None,
                    created_at: document.created_at,
                    updated_at: document.updated_at,
                    chat_created_at: document.created_at,
                    chat_updated_at: document.updated_at,
                })
            }
        }
    }

    /// Up to `limit` units lacking a vector for this generation, with their text.
    pub(crate) fn pending_connector_embeddings(
        &self,
        generation: &str,
        limit: usize,
    ) -> Result<Vec<(i64, String)>, AppError> {
        sql::ConnectorPendingEmbeddings::builder()
            .generation(generation)
            .page_size(i64::try_from(limit).unwrap_or(i64::MAX))
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?
            .into_iter()
            .map(|row| {
                let evidence =
                    self.connector_evidence(unit_kind(&row.kind), &row.source_id, &row.record_id)?;
                Ok((
                    row.id,
                    unit_text(evidence.title.as_deref(), &evidence.content),
                ))
            })
            .collect()
    }

    pub(crate) fn store_connector_embedding(
        &self,
        unit_id: i64,
        generation: &str,
        vector: &[u8],
        norm: f32,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        sql::ConnectorPutEmbedding::builder()
            .unit_id(unit_id)
            .generation(generation)
            .dimensions(
                i64::try_from(vector.len())
                    .map_err(|_| AppError::internal("embedding dimensions overflow"))?,
            )
            .norm(f64::from(norm))
            .vector(vector)
            .build()
            .execute(&self.connection)
            .map_err(AppError::database)?;
        Ok(())
    }

    /// Every unit has a vector of this generation.
    pub(crate) fn connector_embeddings_complete(&self, generation: &str) -> Result<bool, AppError> {
        Ok(sql::ConnectorEmbeddingsComplete::builder()
            .generation(generation)
            .build()
            .query_one(&self.connection)
            .map_err(AppError::database)?
            .complete
            != 0)
    }
}

impl Storage {
    /// A connector message with up to `context` chronological neighbours in its
    /// chat, or a document on its own. Unknown anchors yield nothing.
    pub(crate) fn connector_view(
        &self,
        anchor: &str,
        context: u32,
    ) -> Result<Vec<super::Message>, AppError> {
        let Some((kind, source_id, record_id)) = parse_anchor(anchor) else {
            return Ok(Vec::new());
        };
        match kind {
            "document" => {
                let Some(row) = sql::ConnectorDocumentEvidence::builder()
                    .source_id(source_id)
                    .id(record_id)
                    .build()
                    .query_opt(&self.connection)
                    .map_err(AppError::database)?
                else {
                    return Ok(Vec::new());
                };
                let document: Document = decode(&row.record)?;
                Ok(vec![super::Message {
                    attachments: Vec::new(),
                    id: anchor.to_owned(),
                    ordinal: 0,
                    role: None,
                    author_kind: None,
                    content: unit_text(document.title.as_deref(), &document.content),
                    created_at: document.created_at,
                }])
            }
            "message" => {
                let Some(row) = sql::ConnectorMessageEvidence::builder()
                    .source_id(source_id)
                    .id(record_id)
                    .build()
                    .query_opt(&self.connection)
                    .map_err(AppError::database)?
                else {
                    return Ok(Vec::new());
                };
                let target: Message = decode(&row.message)?;
                let messages = sql::ConnectorChatMessages::builder()
                    .source_id(source_id)
                    .chat_id(target.chat_id.as_str())
                    .build()
                    .query_many(&self.connection)
                    .map_err(AppError::database)?;
                let Some(position) = messages.iter().position(|row| row.id == record_id) else {
                    return Ok(Vec::new());
                };
                let context = usize::try_from(context).unwrap_or(usize::MAX);
                let first = position.saturating_sub(context);
                let last = position.saturating_add(context).min(messages.len() - 1);
                messages[first..=last]
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| {
                        let message: Message = decode(&row.record)?;
                        Ok(super::Message {
                            attachments: Vec::new(),
                            id: self::anchor("message", source_id, message.id.as_str()),
                            ordinal: i64::try_from(first + offset).unwrap_or(i64::MAX),
                            role: None,
                            author_kind: Some(
                                message
                                    .author
                                    .map_or(AuthorKind::Unknown, |author| author.kind),
                            ),
                            content: message.content,
                            created_at: message.created_at,
                        })
                    })
                    .collect()
            }
            _ => Ok(Vec::new()),
        }
    }
}

impl Storage {
    /// The indexed text behind a connector anchor, for contextual reranking.
    pub(crate) fn connector_document(&self, anchor: &str) -> Result<String, AppError> {
        let (kind, source_id, record_id) = parse_anchor(anchor)
            .ok_or_else(|| AppError::database_data("invalid connector anchor"))?;
        let kind = unit_kind(kind);
        let evidence = self.connector_evidence(kind, source_id, record_id)?;
        Ok(unit_text(evidence.title.as_deref(), &evidence.content))
    }
}

fn unit_kind(kind: &str) -> Kind {
    if kind == "document" {
        Kind::Document
    } else {
        Kind::Message
    }
}
