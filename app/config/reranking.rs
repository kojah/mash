//! Explicit placement of contextual reranking. The local CPU reranker is the
//! default and the offline path; a remote worker is opt-in.
use crate::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "backend", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Reranking {
    Local {},
    Remote {
        url: String,
        token_env: String,
        /// How long a search waits for the worker before it reranks on the
        /// local CPU instead. A cold worker takes far longer than this.
        #[serde(default = "default_timeout")]
        timeout_milliseconds: u32,
    },
}

impl Default for Reranking {
    fn default() -> Self {
        Self::Local {}
    }
}

const fn default_timeout() -> u32 {
    1500
}

/// A search should never wait much longer for a remote worker than the local
/// CPU reranker takes to answer it.
const TIMEOUT_MILLISECONDS: std::ops::RangeInclusive<u32> = 100..=10_000;

impl Reranking {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        let Self::Remote {
            url,
            token_env,
            timeout_milliseconds,
        } = self
        else {
            return Ok(());
        };
        super::embeddings::validate_service(url, token_env, "reranking")?;
        if !TIMEOUT_MILLISECONDS.contains(timeout_milliseconds) {
            return Err(AppError::configuration(
                "reranking timeout_milliseconds must be between 100 and 10000",
            ));
        }
        Ok(())
    }
}
