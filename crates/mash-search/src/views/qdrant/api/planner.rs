//! Keep filtered HNSW requests from silently choosing large exact scans.
use super::super::VectorIndex;
use super::{AppError, Client, Info, Method, failure};
use serde::{Deserialize, Serialize};

// Qdrant measures these thresholds in KiB. Ten is its minimum scan threshold;
// at 384 float32 dimensions it permits scans of roughly six vectors per segment.
pub(super) const FULL_SCAN_KIB: usize = 10;
const INDEXING_KIB: usize = 1024;
// Keep bulk load from rebuilding HNSW after each staged batch.
const BULK_INDEXING_KIB: usize = 10_000_000;

#[derive(Default, Deserialize, Serialize)]
pub(super) struct Optimizers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    indexing_threshold: Option<usize>,
}

impl Optimizers {
    pub(super) fn search_default() -> Self {
        Self {
            indexing_threshold: Some(INDEXING_KIB),
        }
    }
}

#[derive(Serialize)]
struct Threshold {
    #[serde(skip_serializing_if = "Option::is_none")]
    full_scan_threshold: Option<usize>,
}

#[derive(Serialize)]
struct Update {
    hnsw_config: Threshold,
    optimizers_config: Optimizers,
}

fn update(info: &Info) -> Update {
    let scan = info.config.hnsw_config.full_scan_threshold;
    let indexing = info.config.optimizers_config.indexing_threshold;
    Update {
        hnsw_config: Threshold {
            full_scan_threshold: scan
                .is_none_or(|v| v > FULL_SCAN_KIB)
                .then_some(FULL_SCAN_KIB),
        },
        optimizers_config: Optimizers {
            indexing_threshold: indexing
                .is_none_or(|v| v == 0 || v > INDEXING_KIB)
                .then_some(INDEXING_KIB),
        },
    }
}

impl Update {
    fn needed(&self) -> bool {
        self.hnsw_config.full_scan_threshold.is_some()
            || self.optimizers_config.indexing_threshold.is_some()
    }
}

#[derive(Serialize)]
struct ThresholdPatch {
    optimizers_config: Optimizers,
}

impl Client {
    /// A bulk loader disables graph construction until its writes are applied.
    pub(in crate::views::qdrant) async fn begin_bulk(&self, name: &str) -> Result<(), AppError> {
        self.set_indexing_threshold(name, BULK_INDEXING_KIB).await
    }

    /// Restore the search threshold once, so the optimizer builds one graph.
    pub(in crate::views::qdrant) async fn end_bulk(&self, name: &str) -> Result<(), AppError> {
        self.set_indexing_threshold(name, INDEXING_KIB).await
    }

    async fn set_indexing_threshold(&self, name: &str, threshold: usize) -> Result<(), AppError> {
        let path = format!("collections/{name}");
        let threshold_patch = ThresholdPatch {
            optimizers_config: Optimizers {
                indexing_threshold: Some(threshold),
            },
        };
        let _: bool = self
            .call(Method::PATCH, &path, Some(&threshold_patch))
            .await?;
        let observed: Info = self.call(Method::GET, &path, None::<&()>).await?;
        if observed.config.optimizers_config.indexing_threshold != Some(threshold) {
            return Err(failure());
        }
        Ok(())
    }

    /// An intentional query activation may schedule a one-time graph rebuild.
    /// Serving and publication do not wait for that background rebuild.
    pub(in crate::views::qdrant) async fn prepare_query(
        &self,
        name: &str,
        policy: VectorIndex,
        activate: bool,
    ) -> Result<(), AppError> {
        if matches!(policy, VectorIndex::Exact {}) {
            return Ok(());
        }
        let path = format!("collections/{name}");
        let info: Info = self.call(Method::GET, &path, None::<&()>).await?;
        let proposed = update(&info);
        if !proposed.needed() {
            return Ok(());
        }
        if !activate {
            return Err(AppError::configuration(
                "existing Qdrant planner permits large scans or delays HNSW indexing; use --activate-query to update search thresholds",
            ));
        }
        let _: bool = self.call(Method::PATCH, &path, Some(&proposed)).await?;
        let observed: Info = self.call(Method::GET, &path, None::<&()>).await?;
        if update(&observed).needed() {
            return Err(failure());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
