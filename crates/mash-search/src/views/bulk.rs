//! Shared lifecycle for whole-node bulk views writes.
//!
//! Local rebuilds and shared replays both stage documents without waiting,
//! publish one awaited views-state point, then restore the collection
//! threshold and settle. The source scan, lock scope, certification decision
//! and progress reporting stay with the caller; this module owns only the
//! common ordering around them.
use super::materialize::Writer;
use super::qdrant::{NodeViews, Store};
use super::scope::Scope;
use super::shared::Build;
use crate::AppError;
use std::future::Future;
use std::path::Path;

/// Who restores the collection threshold and settles after certification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Finish {
    /// The caller writes the final awaited state point; the session restores
    /// the threshold and settles.
    Session,
    /// The caller's merged certification restores the threshold and settles
    /// (shared replay under the group replay lock).
    Certification,
}

/// The differences between local rebuilds and shared replays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkPolicy {
    /// Restore the group threshold even when staging or certification fails.
    restore_on_error: bool,
    finish: Finish,
}

impl BulkPolicy {
    /// Local rebuilds keep caller-owned admission and inline certification;
    /// the session restores the threshold and settles after the final state
    /// point.
    pub const fn local() -> Self {
        Self {
            restore_on_error: false,
            finish: Finish::Session,
        }
    }

    /// Shared replays use merged certification, which owns the threshold
    /// restore and settle. A failed whole-group replay must still lower the
    /// threshold.
    pub const fn shared_replay() -> Self {
        Self {
            restore_on_error: true,
            finish: Finish::Certification,
        }
    }
}

/// A begun bulk views write. Dropping it does not restore the threshold;
/// callers use [`BulkViews::run`] for the normal lifecycle.
pub struct BulkViews<'a> {
    store: &'a Store,
    scope: &'a Scope,
    policy: BulkPolicy,
}

impl<'a> BulkViews<'a> {
    /// Defer graph construction for the whole node write.
    pub async fn begin(
        store: &'a Store,
        scope: &'a Scope,
        policy: BulkPolicy,
    ) -> Result<Self, AppError> {
        store.begin_bulk(scope).await?;
        Ok(Self {
            store,
            scope,
            policy,
        })
    }

    /// Persist stale certification before any document mutation, including
    /// deletions.
    pub async fn invalidate(&self, node: &NodeViews) -> Result<(), AppError> {
        self.store.put_node_views_unwaited(self.scope, node).await
    }

    /// Remove documents for chats no longer present in the snapshot.
    pub async fn retain_chats(
        &self,
        current: impl IntoIterator<Item = String>,
    ) -> Result<(), AppError> {
        self.store
            .retain_node_chats_unwaited(self.scope, current.into_iter().collect())
            .await
    }

    /// Remove every document before a whole-node replay.
    pub async fn delete_documents(&self) -> Result<(), AppError> {
        self.store.delete_node_documents_unwaited(self.scope).await
    }

    /// Stage documents without awaiting each batch. The caller supplies the
    /// barrier through certification.
    pub fn writer<'b>(&'b self, models: &'b Path) -> Writer<'b> {
        Writer::new(self.store, self.scope, Some(models)).with_bulk()
    }

    /// Send the last staged batch and return the materialization summary.
    pub async fn finish(&self, writer: Writer<'_>) -> Result<Build, AppError> {
        writer.finish().await
    }

    /// Run the source-specific staging and certification. On success the
    /// policy decides who restores the threshold and settles; on failure the
    /// policy decides whether the threshold must still be restored.
    pub async fn run<T, F, Fut, P>(&self, work: F, after_restore: P) -> Result<T, AppError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, AppError>>,
        P: FnOnce(&T),
    {
        match work().await {
            Ok(value) => {
                if self.policy.finish == Finish::Session {
                    self.store.end_bulk(self.scope).await?;
                    after_restore(&value);
                    self.store.settle(self.scope).await?;
                }
                Ok(value)
            }
            Err(error) => {
                if self.policy.restore_on_error {
                    let _ = self.store.end_bulk(self.scope).await;
                }
                Err(error)
            }
        }
    }
}
