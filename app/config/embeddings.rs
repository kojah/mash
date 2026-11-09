//! Explicit placement of batch inference. Query inference remains local.
use crate::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "backend", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Embeddings {
    Local {},
    Remote {
        url: String,
        token_env: String,
        #[serde(default = "default_timeout")]
        timeout_seconds: u32,
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

impl Embeddings {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        let Self::Remote {
            url,
            token_env,
            timeout_seconds,
        } = self
        else {
            return Ok(());
        };
        let url = reqwest::Url::parse(url)
            .map_err(|_| AppError::configuration("invalid embedding service URL"))?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AppError::configuration(
                "embedding service requires HTTPS (HTTP only on loopback), without URL credentials, query or fragment",
            ));
        }
        if token_env.is_empty()
            || token_env.as_bytes()[0].is_ascii_digit()
            || !token_env
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(AppError::configuration("invalid embedding token_env"));
        }
        if !(1..=120).contains(timeout_seconds) {
            return Err(AppError::configuration(
                "embedding timeout_seconds must be between 1 and 120",
            ));
        }
        Ok(())
    }
}
