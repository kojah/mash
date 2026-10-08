//! Publish only facts available in the shared freshness protocol.
use super::Freshness;
use crate::{
    AppError,
    core::coverage::{Index, Member, Retained, Scope},
};

impl Freshness {
    pub(crate) fn coverage_members(&self, scope: &Scope) -> Result<Vec<Member>, AppError> {
        let unscoped = scope.provider.is_empty() && scope.after.is_none() && scope.before.is_none();
        self.nodes
            .iter()
            .filter(|node| scope.includes(&node.node_id))
            .map(|node| {
                let conversations = u64::try_from(node.conversations)
                    .map_err(|_| AppError::shared("invalid shared conversation count"))?;
                let messages = u64::try_from(node.messages)
                    .map_err(|_| AppError::shared("invalid shared message count"))?;
                Ok(Member {
                    node_id: node.node_id.clone(),
                    index: Index::Shared,
                    source_silver: node.source_silver.clone(),
                    providers: None,
                    retained: unscoped.then_some(Retained {
                        conversations,
                        messages,
                        earliest_message_at: None,
                        latest_message_at: None,
                    }),
                })
            })
            .collect()
    }
}
