//! Silver counts change only with a committed publication, so freshness and
//! status reuse them per node head instead of scanning silver on every request.
use super::{Identity, Store};
use crate::AppError;
use crate::shared::silver_store;
use crate::storage::Counts;
use std::collections::HashMap;

/// Last counts per (group, node), keyed by the head version they were read with.
#[derive(Default)]
pub(super) struct Cache(std::sync::Mutex<HashMap<(String, String), (String, Counts)>>);

impl Cache {
    fn get(&self, key: &(String, String), version: &str) -> Result<Option<Counts>, AppError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| AppError::internal("shared count cache is poisoned"))?
            .get(key)
            .filter(|(cached, _)| cached == version)
            .map(|(_, counts)| counts.clone()))
    }

    fn put(&self, key: (String, String), version: String, counts: Counts) -> Result<(), AppError> {
        self.0
            .lock()
            .map_err(|_| AppError::internal("shared count cache is poisoned"))?
            .insert(key, (version, counts));
        Ok(())
    }
}

impl Store {
    /// One node's silver counts, recounted only after its head changes.
    pub(super) async fn counts(&self, identity: &Identity) -> Result<Counts, AppError> {
        let key = (identity.group_id.clone(), identity.node_id.clone());
        if let Some(head) = self.head(identity).await?
            && let Some(counts) = self.counts.get(&key, &head.version)?
        {
            return Ok(counts);
        }
        // Read both from one snapshot so the cached counts belong to that head.
        let (head, counts) = self.read(identity, silver_store::counted_head).await?;
        if let Some(head) = head {
            self.counts.put(key, head.version, counts.clone())?;
        }
        Ok(counts)
    }
}
