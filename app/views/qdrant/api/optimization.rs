//! Check health without making publication wait for collection-wide maintenance.
use super::super::VectorIndex;
use super::{AppError, Client, Method};
use serde::Deserialize;

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

impl Client {
    /// Exercise group vector/filter pages and its selected retrieval path.
    #[tracing::instrument(name = "qdrant_warm_search", skip_all)]
    pub(in crate::views::qdrant) async fn warm_search(
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

    /// Awaited point writes establish application, not optimizer quiescence.
    /// Yellow is normal background indexing; other nodes must not delay this
    /// publication by continuing to write into the same collection.
    #[tracing::instrument(name = "qdrant_check_search_health", skip_all)]
    pub(in crate::views::qdrant) async fn check_search_health(
        &self,
        name: &str,
    ) -> Result<(), AppError> {
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
            Status::Green | Status::Yellow => Ok(()),
            Status::Red | Status::Grey => Err(AppError::shared(
                "shared publication committed but Qdrant optimization is failed or paused; inspect backend health",
            )),
        }
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
