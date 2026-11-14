//! Local views destination. Credentials are resolved only when opening Qdrant.
use crate::AppError;
use crate::views::qdrant::{MemoryTier, VectorIndex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct QdrantConfig {
    pub url: String,
    pub grpc_url: String,
    pub api_key_env: Option<String>,
    pub memory: MemoryTier,
    pub vector_search: VectorIndex,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            url: "http://127.0.0.1:6333".into(),
            grpc_url: "http://127.0.0.1:6334".into(),
            api_key_env: None,
            memory: MemoryTier::default(),
            vector_search: VectorIndex::default(),
        }
    }
}

impl QdrantConfig {
    pub(super) fn validate(&self) -> Result<(), AppError> {
        crate::views::qdrant::endpoint(&self.url)?;
        crate::views::qdrant::endpoint(&self.grpc_url)?;
        self.vector_search.validate()?;
        if let Some(name) = &self.api_key_env
            && (name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || name.as_bytes()[0].is_ascii_digit())
        {
            return Err(AppError::configuration("invalid Qdrant api_key_env"));
        }
        Ok(())
    }
}
