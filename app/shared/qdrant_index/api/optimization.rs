//! Publication owns the optimizer drain; interactive searches retain their budget.
use super::super::VectorIndex;
use super::{AppError, Client, Method};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Green,
    Yellow,
    Grey,
    Red,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Healthy {
    Ok,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Optimizer {
    Healthy(Healthy),
    Failed { error: serde::de::IgnoredAny },
}

#[derive(Deserialize)]
struct Health {
    status: Status,
    optimizer_status: Optimizer,
}

#[derive(Deserialize)]
struct Summary {
    queued_optimizations: usize,
}

#[derive(Deserialize)]
struct Progress {
    summary: Summary,
    // Individual jobs contain diagnostics we neither expose nor need to decode.
    running: Vec<serde::de::IgnoredAny>,
}

impl Client {
    /// Exercise group vector/filter pages and its selected retrieval path.
    #[tracing::instrument(name = "qdrant_warm_search", skip_all)]
    pub(in crate::shared::qdrant_index) async fn warm_search(
        &self,
        name: &str,
        using: &str,
        dimensions: usize,
        nodes: &[String],
        policy: VectorIndex,
    ) -> Result<(), AppError> {
        let mut vector = vec![0.0; dimensions];
        let Some(first) = vector.first_mut() else {
            return Err(super::failure());
        };
        *first = 1.0;
        let mut request = super::QueryRequest {
            query: super::QueryVector::Dense(vector),
            using: using.into(),
            filter: super::Filter {
                must: vec![super::matches("node_id", nodes.to_vec())],
                ..super::Filter::default()
            },
            params: super::Params {
                exact: true,
                hnsw_ef: 200,
            },
            limit: 1,
            with_payload: false,
        };
        self.query::<Option<serde::de::IgnoredAny>>(name, &request)
            .await
            .map_err(|_| probe_failed())?;
        if let VectorIndex::Hnsw { ef_runtime } = policy {
            request.params.exact = false;
            request.params.hnsw_ef = ef_runtime;
            request.limit = 200;
            self.query::<Option<serde::de::IgnoredAny>>(name, &request)
                .await
                .map_err(|_| probe_failed())?;
        }
        Ok(())
    }

    #[tracing::instrument(name = "qdrant_settle_search", skip_all)]
    pub(in crate::shared::qdrant_index) async fn settle_search(
        &self,
        name: &str,
    ) -> Result<(), AppError> {
        self.settle_search_with_budget(name, Duration::from_secs(300), Duration::from_secs(1))
            .await
    }

    async fn settle_search_with_budget(
        &self,
        name: &str,
        budget: Duration,
        interval: Duration,
    ) -> Result<(), AppError> {
        tokio::time::timeout(budget, async {
            loop {
                let health: Health = self
                    .call(Method::GET, &format!("collections/{name}"), None::<&()>)
                    .await
                    .map_err(|_| unavailable())?;
                if let Optimizer::Failed { error: _error } = health.optimizer_status {
                    return Err(AppError::shared(
                        "shared publication committed but Qdrant optimization failed; inspect backend health",
                    ));
                }
                match health.status {
                    Status::Red | Status::Grey => {
                        return Err(AppError::shared(
                            "shared publication committed but Qdrant optimization is failed or paused; inspect backend health",
                        ));
                    }
                    Status::Green => {
                        let progress: Progress = self
                            .call(
                                Method::GET,
                                &format!("collections/{name}/optimizations"),
                                None::<&()>,
                            )
                            .await
                            .map_err(|_| unavailable())?;
                        if progress.summary.queued_optimizations == 0 && progress.running.is_empty() {
                            return Ok(());
                        }
                    }
                    Status::Yellow => {}
                }
                tokio::time::sleep(interval).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            Err(AppError::shared(
                "shared publication committed but Qdrant is still optimizing; check shared status before retrying",
            ))
        })
    }
}

fn probe_failed() -> AppError {
    AppError::shared(
        "shared publication committed but dense readiness probe failed; check shared status",
    )
}

fn unavailable() -> AppError {
    AppError::shared(
        "shared publication committed but Qdrant optimization status is unavailable; check shared status",
    )
}

#[cfg(test)]
mod tests;
