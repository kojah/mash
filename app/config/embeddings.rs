//! Explicit placement of batch inference. Query inference remains local.
use crate::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "backend", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Embeddings {
    Local {},
    Remote {
        url: String,
        token_env: String,
        #[serde(default = "default_timeout")]
        timeout_seconds: u32,
        /// Batches with fewer new texts than this use the local model, so a
        /// routine refresh does not wait for a remote worker to start. Zero
        /// sends every batch to the worker.
        #[serde(default)]
        local_below: u32,
    },
}

impl Default for Embeddings {
    fn default() -> Self {
        Self::Local {}
    }
}

const fn default_timeout() -> u32 {
    120
}

/// The most new texts one flushed group can hold, so a larger threshold could
/// never select the worker.
const MAX_LOCAL_BELOW: u32 = 1024;
const _: () = assert!(
    MAX_LOCAL_BELOW as usize
        == crate::semantic::remote::MAX_INPUTS * crate::semantic::remote::REQUEST_CONCURRENCY
);

impl Embeddings {
    /// Whether a batch of this many new texts is embedded by the local model.
    pub(crate) const fn runs_locally(&self, inputs: usize) -> bool {
        match self {
            Self::Local {} => true,
            Self::Remote { local_below, .. } => inputs < *local_below as usize,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), AppError> {
        let Self::Remote {
            url,
            token_env,
            timeout_seconds,
            local_below,
        } = self
        else {
            return Ok(());
        };
        validate_service(url, token_env, "embedding")?;
        if !(1..=120).contains(timeout_seconds) {
            return Err(AppError::configuration(
                "embedding timeout_seconds must be between 1 and 120",
            ));
        }
        if *local_below > MAX_LOCAL_BELOW {
            return Err(AppError::configuration(
                "embedding local_below must be at most 1024; use backend = \"local\" to embed everything locally",
            ));
        }
        Ok(())
    }
}

/// An authenticated inference service: HTTPS (HTTP only on loopback), no URL
/// credentials, query or fragment, and a token named by an environment variable.
pub(super) fn validate_service(url: &str, token_env: &str, service: &str) -> Result<(), AppError> {
    let url = reqwest::Url::parse(url)
        .map_err(|_| AppError::configuration(format!("invalid {service} service URL")))?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::configuration(format!(
            "{service} service requires HTTPS (HTTP only on loopback), without URL credentials, query or fragment"
        )));
    }
    if token_env.is_empty()
        || token_env.as_bytes()[0].is_ascii_digit()
        || !token_env
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(AppError::configuration(format!(
            "invalid {service} token_env"
        )));
    }
    Ok(())
}
