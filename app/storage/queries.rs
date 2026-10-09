use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::generated as sql;
use super::schema::SCHEMA_VERSION;
use super::{Counts, Message, SearchHit, SearchUpdates, StatusSnapshot, Storage, cutoff_timestamp};
use crate::AppError;

impl Storage {
    pub(crate) fn open_search(path: &Path, generation: &str) -> Result<Self, AppError> {
        let storage = Self::open_catalog(path)?;
        if storage.derived_search_is_dirty()? || !storage.semantic_index_is_ready(generation)? {
            return Err(AppError::search_not_ready(
                "initial indexing has not completed or the index needs updating; run `mash index`",
            ));
        }
        Ok(storage)
    }

    /// Current schema only: never initialize, migrate or require models.
    pub(crate) fn open_catalog(path: &Path) -> Result<Self, AppError> {
        if !path.is_file() {
            return Err(AppError::missing_database(path));
        }
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::database)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(AppError::database)?;
        if version == 0 {
            return Err(AppError::search_not_ready(
                "index requires an explicit rebuild; run `mash index`",
            ));
        }
        if version != SCHEMA_VERSION {
            return Err(super::schema::unsupported(version));
        }
        super::schema::attach_gold(&connection, &super::schema::gold_path(path), false)?;
        Ok(Self {
            attachments_config: crate::attachments::Configuration::default(),
            connection,
            writer_active: false,
            search_updates: SearchUpdates::Immediate,
            transaction_base_messages: 0,
        })
    }

    pub(crate) fn open_existing(path: &Path) -> Result<Self, AppError> {
        if !path.is_file() {
            return Err(AppError::missing_database(path));
        }
        Self::open(path)
    }

    pub(crate) fn status_snapshot(
        path: &Path,
        generation: &str,
    ) -> Result<StatusSnapshot, AppError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::database)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(AppError::database)?;
        if version == 0 {
            return Ok(StatusSnapshot {
                lexical: None,
                gold: None,
                silver: None,
                counts: Counts::default(),
                current_embeddings: 0,
                derived_clean: false,
                exact_semantic_coverage: false,
            });
        }
        if version != SCHEMA_VERSION {
            return Err(super::schema::unsupported(version));
        }
        super::schema::attach_gold(&connection, &super::schema::gold_path(path), false)?;
        let storage = Self {
            attachments_config: crate::attachments::Configuration::default(),
            connection,
            writer_active: false,
            search_updates: SearchUpdates::Immediate,
            transaction_base_messages: 0,
        };
        Ok(StatusSnapshot {
            lexical: storage.lexical_manifest()?,
            gold: storage.gold_manifest()?,
            silver: Some(storage.silver_certificate()?),
            counts: storage.counts()?,
            current_embeddings: storage.embedding_count(generation)?,
            derived_clean: !storage.derived_search_is_dirty()?,
            exact_semantic_coverage: storage.semantic_index_is_ready(generation)?,
        })
    }

    pub(crate) fn counts(&self) -> Result<Counts, AppError> {
        let counts = sql::Counts
            .query_one(&self.connection)
            .map_err(AppError::database)?;
        Ok(Counts {
            searchable_user_messages: None,
            conversations: u64::try_from(counts.conversations)
                .map_err(|_| AppError::internal("negative conversation count"))?,
            messages: u64::try_from(counts.messages)
                .map_err(|_| AppError::internal("negative message count"))?,
            searchable_messages: u64::try_from(counts.searchable_messages)
                .map_err(|_| AppError::internal("negative searchable message count"))?,
            embeddings: u64::try_from(counts.embeddings)
                .map_err(|_| AppError::internal("negative embedding count"))?,
        })
    }

    #[cfg(test)]
    pub(crate) fn search(
        &self,
        query: &str,
        limit: usize,
        provider: Option<crate::provider::Provider>,
        days: Option<u32>,
    ) -> Result<Vec<SearchHit>, AppError> {
        self.search_filtered(query, limit, provider, days, None)
    }

    pub(crate) fn search_filtered(
        &self,
        query: &str,
        limit: usize,
        provider: Option<crate::provider::Provider>,
        days: Option<u32>,
        filters: Option<&crate::search_filter::Scope>,
    ) -> Result<Vec<SearchHit>, AppError> {
        if !crate::search_query::has_terms(query) {
            return Err(AppError::usage(
                "search query must contain letters or numbers",
            ));
        }
        let mode = filters.map_or(crate::gold::matching::Mode::Strict, |f| f.match_mode);
        let plan = self.lexical_plan(query, mode, filters)?;
        let original_query = crate::search_query::sqlite(query);
        let expanded = plan.sqlite();
        let query = if mode == crate::gold::matching::Mode::Strict || expanded.is_empty() {
            original_query.clone()
        } else {
            format!("({original_query}) OR ({expanded})")
        };
        let limit = i64::try_from(limit.min(1_000)).unwrap_or(1_000);
        let cutoff = cutoff_timestamp(days)?;
        let predicate = filters.map_or_else(|| "TRUE".to_owned(), |f| f.filter.sql(Some(&f.node)));
        // Reuse the generated projection verbatim so its decoder stays aligned
        // when fields are added or reordered. sqlc cannot analyze FTS MATCH's
        // table operand; only that ranking/filtering clause is handwritten.
        let (projection, _) = sql::SearchHit::QUERY
            .split_once("FROM messages m")
            .ok_or_else(|| AppError::internal("invalid generated search projection"))?;
        let mut statement = self
            .connection
            .prepare(&format!(
                "{projection}, bm25(message_fts) AS lexical_score
                   FROM message_fts
                   JOIN messages m ON m.id = message_fts.message_id
                   JOIN conversations c ON c.id = m.conversation_id
                   LEFT JOIN conversation_enrichment e ON e.conversation_id = c.id
                  WHERE message_fts MATCH ?1
                    AND (?2 IS NULL OR c.provider = ?2)
                    AND (?3 IS NULL OR m.created_at >= ?3)
                    AND ({predicate})
                  ORDER BY CASE WHEN message_fts.rowid IN
                    (SELECT rowid FROM message_fts WHERE message_fts MATCH ?5)
                    THEN 0 ELSE 1 END, lexical_score, m.id
                  LIMIT ?4"
            ))
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(
                params![query, provider, cutoff, limit, original_query],
                |row| {
                    Ok((
                        sql::SearchHitRow::from_row(row)?,
                        row.get::<_, f64>("lexical_score")?,
                    ))
                },
            )
            .map_err(AppError::database)?;
        let mut hits = rows
            .map(|row| {
                let (row, score) = row.map_err(AppError::database)?;
                let mut hit = search_hit(row)?;
                hit.lexical_score = Some(score);
                hit.lexical_match = plan.evidence(&crate::redaction::text(&hit.content), mode);
                Ok(hit)
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        // A provider selection names native history only.
        if provider.is_none() {
            let limit = usize::try_from(limit).unwrap_or(usize::MAX);
            let ranked = self.search_connector_lexical(&query, limit, cutoff, filters)?;
            let ids = ranked.iter().map(|(id, _)| *id).collect::<Vec<_>>();
            let mut connector = Vec::with_capacity(ids.len());
            for (mut hit, (_, score)) in self.connector_hits(&ids)?.into_iter().zip(ranked) {
                hit.lexical_score = Some(score);
                hit.lexical_match = plan.evidence(&crate::redaction::text(&hit.content), mode);
                connector.push(hit);
            }
            hits = merge_lexical(hits, connector, limit);
        }
        Ok(hits)
    }

    pub(crate) fn search_hits(&self, message_rowids: &[i64]) -> Result<Vec<SearchHit>, AppError> {
        let mut statement = self
            .connection
            .prepare_cached(sql::SearchHit::QUERY)
            .map_err(AppError::database)?;
        let mut hits = Vec::with_capacity(message_rowids.len());
        for message_rowid in message_rowids {
            let hit = statement
                .query_row([message_rowid], sql::SearchHitRow::from_row)
                .optional()
                .map_err(AppError::database)?;
            let hit = hit.ok_or_else(|| {
                AppError::database_data("semantic index references a missing message")
            })?;
            hits.push(search_hit(hit)?);
        }
        Ok(hits)
    }

    pub(crate) fn search_documents(&self, message_ids: &[&str]) -> Result<Vec<String>, AppError> {
        let mut statement = self
            .connection
            .prepare_cached(sql::SearchDocument::QUERY)
            .map_err(AppError::database)?;
        message_ids
            .iter()
            .map(|message_id| {
                if message_id.starts_with(super::connector_gold::ANCHOR_PREFIX) {
                    return self.connector_document(message_id);
                }
                statement
                    .query_row([message_id], sql::SearchDocumentRow::from_row)
                    .map(|row| row.content)
                    .map_err(AppError::database)
            })
            .collect()
    }

    pub(crate) fn view(&self, id: &str, context: u32) -> Result<Vec<Message>, AppError> {
        self.view_content(id, context, super::MessageContent::Canonical)
    }

    pub(crate) fn view_content(
        &self,
        id: &str,
        context: u32,
        representation: super::MessageContent,
    ) -> Result<Vec<Message>, AppError> {
        if id.starts_with(super::connector_gold::ANCHOR_PREFIX) {
            return self.connector_view(id, context);
        }
        let anchor = sql::Anchor::builder()
            .id(id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?;
        let Some(anchor) = anchor else {
            return Ok(Vec::new());
        };
        let context = i64::from(context);
        if anchor.context_parent.is_some() {
            return self.view_branch(&anchor, context, representation);
        }
        let rows = sql::ViewMessages::builder()
            .conversation_id(&anchor.conversation_id)
            .first_ordinal(anchor.ordinal.saturating_sub(context))
            .last_ordinal(anchor.ordinal.saturating_add(context))
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?;
        Ok(rows
            .into_iter()
            .map(|row| Message {
                attachments: Vec::new(),
                id: row.id,
                ordinal: row.ordinal,
                role: Some(row.role.into()),
                author_kind: None,
                content: match representation {
                    super::MessageContent::Canonical => row.content,
                    super::MessageContent::SearchProjection => row.projection,
                },
                created_at: row.created_at,
            })
            .collect())
    }

    fn view_branch(
        &self,
        anchor: &sql::AnchorRow,
        context: i64,
        representation: super::MessageContent,
    ) -> Result<Vec<Message>, AppError> {
        let read = |ordinal| {
            sql::ContextMessage::builder()
                .conversation_id(&anchor.conversation_id)
                .ordinal(ordinal)
                .build()
                .query_opt(&self.connection)
                .map_err(AppError::database)
        };
        let mut rows = Vec::new();
        let mut ordinal = anchor.ordinal;
        for _ in 0..=context {
            let Some(row) = read(ordinal)? else { break };
            let parent = row.context_parent;
            rows.push(row);
            let Some(parent) = parent.filter(|n| *n >= 0 && *n < ordinal) else {
                break;
            };
            ordinal = parent;
        }
        ordinal = anchor.ordinal;
        for _ in 0..context {
            let Some(child) = sql::FirstChild::builder()
                .conversation_id(&anchor.conversation_id)
                .parent(Some(ordinal))
                .build()
                .query_opt(&self.connection)
                .map_err(AppError::database)?
            else {
                break;
            };
            if child.ordinal <= ordinal {
                return Err(AppError::database_data("invalid message context parent"));
            }
            ordinal = child.ordinal;
            let Some(row) = read(ordinal)? else { break };
            rows.push(row);
        }
        rows.sort_by_key(|row| row.ordinal);
        Ok(rows
            .into_iter()
            .map(|row| Message {
                attachments: Vec::new(),
                id: row.id,
                ordinal: row.ordinal,
                role: Some(row.role.into()),
                author_kind: None,
                content: match representation {
                    super::MessageContent::Canonical => row.content,
                    super::MessageContent::SearchProjection => row.projection,
                },
                created_at: row.created_at,
            })
            .collect())
    }
}

/// Interleave two ranked lists by BM25 (lower is better), keeping each list's
/// own order: native results rank exact-phrase matches ahead of their scores.
fn merge_lexical(
    native: Vec<SearchHit>,
    connector: Vec<SearchHit>,
    limit: usize,
) -> Vec<SearchHit> {
    let score = |hit: &SearchHit| hit.lexical_score.unwrap_or(f64::INFINITY);
    let mut merged = Vec::with_capacity(limit.min(native.len() + connector.len()));
    let mut native = native.into_iter().peekable();
    let mut connector = connector.into_iter().peekable();
    while merged.len() < limit {
        let take_native = match (native.peek(), connector.peek()) {
            (Some(left), Some(right)) => score(left) <= score(right),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        merged.extend(if take_native {
            native.next()
        } else {
            connector.next()
        });
    }
    merged
}

pub(super) fn search_hit(row: sql::SearchHitRow) -> Result<SearchHit, AppError> {
    Ok(SearchHit {
        id: row.id,
        conversation_id: row.conversation_id,
        source: crate::source::Source::Provider(row.provider.parse()?),
        kind: crate::source::Kind::Message,
        role: Some(row.role.into()),
        author_kind: None,
        content: row.content,
        copies: Vec::new(),
        created_at: row.created_at,
        metadata: crate::search_results::ChatMetadata {
            enrichment: row
                .enrichment
                .map(|json| super::enrichment::decode(&json))
                .transpose()?,
            title: row.title,
            project: row.project,
            repository: row.repository,
            created_at: row.conversation_created_at,
            updated_at: row.conversation_updated_at,
        },
        lexical_score: None,
        lookup: None,
        lexical_match: None,
        semantic_score: None,
        fusion_score: 0.0,
        rerank_score: None,
        node_id: None,
    })
}
