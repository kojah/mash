//! Remove one node's published history on the operator's explicit request.
use super::{Identity, Store};
use crate::AppError;
use crate::shared::silver_store;
use crate::views::qdrant::NodeViews;

/// Per-store interruption points; no global state or production fault switches.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetireFault {
    AfterInvalidation,
    AfterDocuments,
}

#[cfg(test)]
impl Store {
    pub(crate) fn with_retire_fault(mut self, fault: RetireFault) -> Self {
        self.retire_fault = Some(fault);
        self
    }

    fn interrupt_retire(&self, point: RetireFault) -> Result<(), AppError> {
        if self.retire_fault == Some(point) {
            return Err(AppError::internal("injected retire interruption"));
        }
        Ok(())
    }
}

/// What a retirement removed.
#[derive(Debug, serde::Serialize)]
pub(crate) struct Retired {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) chats: u64,
    pub(crate) messages: u64,
}

impl Store {
    /// Delete everything the node published: its search documents, then its
    /// history. The views are marked uncertified first, so an interruption
    /// leaves a node that reports stale views and can be retired again; it
    /// never leaves search documents for history that is gone.
    pub(crate) async fn retire(&self, identity: &Identity) -> Result<Retired, AppError> {
        let _node = self.lock(identity).await?;
        let _replay = self.replay_lock(&identity.group_id).await?;
        let counts = self.read(identity, silver_store::counts).await?;
        {
            let _write = self.views_write_guard(identity).await?;
            self.views
                .put_node_views(identity, &NodeViews::empty(&self.views, identity))
                .await?;
        }
        #[cfg(test)]
        self.interrupt_retire(RetireFault::AfterInvalidation)?;
        self.views.delete_node_documents(identity).await?;
        #[cfg(test)]
        self.interrupt_retire(RetireFault::AfterDocuments)?;
        let silver = self.silver.clone();
        let node = identity.clone();
        crate::blocking(move || silver.retire(&node)).await?;
        Ok(Retired {
            group_id: identity.group_id.clone(),
            node_id: identity.node_id.clone(),
            chats: counts.chats,
            messages: counts.messages,
        })
    }
}
