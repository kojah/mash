//! Active query policy, independent of canonical history and build identity.
use crate::{AppError, search_results::SearchUnit};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Configuration {
    pub(crate) query: Query,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Query {
    pub(crate) unit: SearchUnit,
    pub(crate) limit: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            unit: SearchUnit::Threads,
            limit: 20,
        }
    }
}

impl Configuration {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if !(1..=200).contains(&self.query.limit) {
            return Err(AppError::configuration(
                "gold query limit must be between 1 and 200",
            ));
        }
        Ok(())
    }
}

impl Query {
    pub(crate) fn resolve(
        &self,
        limit: Option<usize>,
        unit: Option<SearchUnit>,
    ) -> (usize, SearchUnit) {
        (limit.unwrap_or(self.limit), unit.unwrap_or(self.unit))
    }
}
