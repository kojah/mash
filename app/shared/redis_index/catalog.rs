//! Read immutable metadata catalogs without fetching canonical messages or vectors.
use super::{
    AppError, ConversationRecord, Head, Identity, Store, catalog_key, conflict, decode, prefix,
    redis_error,
};
use crate::thread_catalog::{Collector, Page, Request, Thread};
use std::collections::BTreeMap;

impl Store {
    async fn catalog_heads(
        &self,
        identity: &Identity,
        input: &Request,
    ) -> Result<BTreeMap<String, Head>, AppError> {
        let raw: BTreeMap<String, String> = redis::cmd("HGETALL")
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        raw.into_iter()
            .filter(|(node, _)| input.admits_node(node, &identity.node_id))
            .map(|(node, raw)| Ok((node, decode(&raw)?)))
            .collect()
    }

    pub(in crate::shared) async fn list_threads(
        &self,
        identity: &Identity,
        input: Request,
    ) -> Result<Page, AppError> {
        input.validate()?;
        let heads = self.catalog_heads(identity, &input).await?;
        self.catalog_page(identity, input, heads).await
    }

    pub(super) async fn catalog_page(
        &self,
        identity: &Identity,
        input: Request,
        heads: BTreeMap<String, Head>,
    ) -> Result<Page, AppError> {
        let mut collector = Collector::new(input.clone())?;
        let result = async {
            for (node, head) in &heads {
                if !head.catalog_ready {
                    return Err(AppError::shared("shared thread catalog is unavailable; run `mash index` on the publishing node"));
                }
                let received = self.read_payload(&catalog_key(&identity.group_id, &head.version)).await?;
                let node = node.clone();
                collector = crate::blocking(move || {
                    let rows: BTreeMap<String, ConversationRecord> = received.decode()?;
                    for (id, row) in rows {
                        if id != row.id { return Err(AppError::shared("inconsistent Redis thread catalog identity")); }
                        collector.push(Thread { conversation_id: row.id, provider: row.provider, node_id: node.clone(),
                            metadata: crate::search_results::ThreadMetadata {
                                enrichment: row.enrichment,
                                title: row.title, project: row.project, repository: row.repository,
                                created_at: row.created_at, updated_at: row.updated_at,
                            } });
                    }
                    Ok(collector)
                }).await?;
            }
            Ok(collector.finish())
        }.await;
        let current = self.catalog_heads(identity, &input).await?;
        if !heads
            .iter()
            .map(|(node, head)| (node, &head.version))
            .eq(current.iter().map(|(node, head)| (node, &head.version)))
        {
            return Err(conflict(
                "shared thread catalogs changed while reading; retry the operation",
            ));
        }
        result
    }
}
