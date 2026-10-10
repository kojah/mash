//! Query-only dense vector policy, shared by transport and server configuration.
use crate::AppError;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(tag = "algorithm", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Search {
    Exact {},
    Hnsw {
        #[serde(default = "default_ef")]
        #[schemars(range(min = 1, max = 4096))]
        ef_runtime: usize,
    },
}
impl Default for Search {
    fn default() -> Self {
        Self::Hnsw {
            ef_runtime: default_ef(),
        }
    }
}

const fn default_ef() -> usize {
    200
}
impl Search {
    pub(crate) fn validate(self) -> Result<(), AppError> {
        if matches!(self, Self::Hnsw { ef_runtime } if !(1..=4096).contains(&ef_runtime)) {
            return Err(AppError::configuration(
                "Qdrant ef_runtime must be between 1 and 4096",
            ));
        }
        Ok(())
    }
    pub(crate) fn validate_request(self) -> Result<(), AppError> {
        self.validate()
            .map_err(|_| AppError::usage("shared vector ef_runtime must be between 1 and 4096"))
    }
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Algorithm {
    Exact,
    Hnsw,
}
impl Algorithm {
    pub(crate) fn request(self, ef: Option<usize>) -> Result<Search, AppError> {
        let policy = match (self, ef) {
            (Self::Exact, None) => Search::Exact {},
            (Self::Exact, Some(_)) => {
                return Err(AppError::usage(
                    "shared-hnsw-ef requires shared-vector-search hnsw",
                ));
            }
            (Self::Hnsw, ef) => Search::Hnsw {
                ef_runtime: ef.unwrap_or(default_ef()),
            },
        };
        policy.validate_request()?;
        Ok(policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_search_defaults_to_hnsw_with_explicit_exact_available() {
        assert_eq!(Search::default(), Search::Hnsw { ef_runtime: 200 });
        assert_eq!(
            serde_json::from_str::<Search>(r#"{"algorithm":"hnsw"}"#).unwrap(),
            Search::default()
        );
        assert_eq!(
            serde_json::from_str::<Search>(r#"{"algorithm":"exact"}"#).unwrap(),
            Search::Exact {}
        );
    }
}
