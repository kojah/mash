//! Bounded metadata query through sqlc; no message, vector or model reads.
use super::{Storage, generated};
use crate::AppError;
use crate::search_results::ThreadMetadata;
use crate::thread_catalog::{Direction, Request, Sort, Thread};

impl Storage {
    #[tracing::instrument(name = "local_thread_catalog", skip_all)]
    pub(crate) fn thread_candidates(
        &self,
        request: &Request,
        node: &str,
    ) -> Result<Vec<Thread>, AppError> {
        request.validate()?;
        if !request.admits_node(node, node) {
            return Ok(Vec::new());
        }
        let providers = serde_json::to_string(&request.provider)
            .map_err(|_| AppError::internal("serialize thread provider selection"))?;
        let sort = match request.sort {
            Sort::UpdatedAt => "updated_at",
            Sort::CreatedAt => "created_at",
            Sort::Title => "title",
        };
        let size = i64::try_from(request.offset + request.limit + 1)
            .map_err(|_| AppError::usage("thread page is too large"))?;
        generated::ListThreads::builder()
            .all_providers(i64::from(request.provider.is_empty()))
            .providers_json(&providers)
            .title_contains(request.title_contains.as_deref())
            .after_ms(request.after.map(|time| time * 1000))
            .before_ms(request.before.map(|time| time * 1000))
            .sort_key(sort)
            .descending(i64::from(matches!(request.direction, Direction::Desc)))
            .page_offset(0)
            .page_size(size)
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?
            .into_iter()
            .map(|row| {
                Ok(Thread {
                    conversation_id: row.id,
                    provider: row.provider.parse()?,
                    node_id: node.into(),
                    metadata: ThreadMetadata {
                        enrichment: row
                            .enrichment
                            .map(|json| super::enrichment::decode(&json))
                            .transpose()?,
                        title: row.title,
                        project: row.project,
                        repository: row.repository,
                        created_at: row.created_at,
                        updated_at: row.updated_at,
                    },
                })
            })
            .collect()
    }
}
