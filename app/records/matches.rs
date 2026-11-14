use super::{Body, Review, Scope};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub scope: Scope,
    pub body: Body,
}

#[derive(Debug, Serialize)]
pub struct Match {
    pub id: String,
    pub revision: i64,
    pub statement: String,
    /// Equal interpretation fields; recording-agent attribution is excluded.
    pub exact: bool,
    pub current: bool,
    pub review: Review,
    pub evidence_current: bool,
    pub overlap_percent: u32,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub suggestions: Vec<Match>,
    pub scan_limited: bool,
    pub truncated: bool,
}
