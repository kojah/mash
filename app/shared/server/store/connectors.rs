//! Connector silver commits share the authenticated node's database and writer lock.
use super::{Identity, Store};
use crate::{
    AppError,
    connectors::{Batch, Receipt, silver},
};

impl Store {
    pub(in crate::shared) async fn ingest_connector(
        &self,
        identity: &Identity,
        batch: Batch,
    ) -> Result<Receipt, AppError> {
        let _node = self.lock(identity).await?;
        let store = self.silver.clone();
        let scope = identity.clone();
        crate::blocking(move || {
            store.write(&scope, |transaction| {
                silver::apply(transaction, &batch).map_err(silver::app_error)
            })
        })
        .await
    }

    pub(super) async fn connector_inputs(&self, identity: &Identity) -> Result<bool, AppError> {
        self.read(identity, |connection| {
            crate::storage::generated::ConnectorInputs
                .query_one(connection)
                .map(|row| row.present != 0)
                .map_err(AppError::database)
        })
        .await
    }

    /// Temporary readiness gate until source-neutral gold projection is complete.
    pub(in crate::shared) async fn require_connector_gold(
        &self,
        identity: &Identity,
        exclude_self: bool,
    ) -> Result<(), AppError> {
        for node in self.silver.nodes(&identity.group_id) {
            if exclude_self && node == &identity.node_id {
                continue;
            }
            let scope = Identity {
                node_id: node.clone(),
                ..identity.clone()
            };
            if self.connector_inputs(&scope).await? {
                return Err(AppError::search_not_ready(
                    "external connector gold projection has not completed",
                ));
            }
        }
        Ok(())
    }
}
