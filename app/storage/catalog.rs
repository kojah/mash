//! Bounded metadata query through sqlc; no message, vector or model reads.
use super::{Storage, generated};
use crate::AppError;
use crate::chat_catalog::{Chat, Direction, Request, Sort};
use crate::search_results::ChatMetadata;

impl Storage {
    #[tracing::instrument(name = "local_chat_catalog", skip_all)]
    pub(crate) fn chat_candidates(
        &self,
        request: &Request,
        node: &str,
    ) -> Result<Vec<Chat>, AppError> {
        request.validate()?;
        if !request.admits_node(node, node) {
            return Ok(Vec::new());
        }
        let sort = match request.sort {
            Sort::UpdatedAt => "updated_at",
            Sort::CreatedAt => "created_at",
            Sort::Title => "title",
        };
        let size = i64::try_from(request.offset + request.limit + 1)
            .map_err(|_| AppError::usage("thread page is too large"))?;
        let mut chats = self.native_chat_candidates(request, node, sort, size)?;
        // A provider selection names native history only. The caller's collector
        // applies the remaining filters, ordering and page window.
        if request.provider.is_empty() {
            chats.extend(super::connector_views::chats(&self.connection, node)?);
        }
        Ok(chats)
    }

    fn native_chat_candidates(
        &self,
        request: &Request,
        node: &str,
        sort: &str,
        size: i64,
    ) -> Result<Vec<Chat>, AppError> {
        let providers = serde_json::to_string(&request.provider)
            .map_err(|_| AppError::internal("serialize thread provider selection"))?;
        generated::ListChats::builder()
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
                let provider: crate::provider::Provider = row.provider.parse()?;
                Ok(Chat {
                    conversation_id: row.id,
                    source: crate::source::Source::Provider(provider),
                    object: crate::source::ObjectType::native_chat(provider),
                    ancestors: Vec::new(),
                    node_id: node.into(),
                    metadata: ChatMetadata {
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
