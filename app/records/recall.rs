//! Small, explicitly scoped context packets for agent sessions.
use super::{Body, Scope};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub scope: Scope,
    pub query: Option<String>,
    /// Exact project-relative path; never a basename or a host path.
    pub file: Option<String>,
    #[serde(default)]
    pub include_personal: bool,
    #[serde(default)]
    pub include_review: bool,
    pub limit: Option<usize>,
    pub byte_budget: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Current,
    Candidate,
    StaleEvidence,
    Inactive,
}

#[derive(Debug, Serialize)]
pub(crate) struct Item {
    pub id: String,
    pub owner: String,
    pub scope: Scope,
    pub revision: i64,
    pub body: Body,
    pub status: Status,
    pub confirmations: u32,
    pub last_confirmed_at: Option<i64>,
    pub score: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct Response {
    pub records: Vec<Item>,
    pub scanned: usize,
    /// More records may exist outside the bounded candidate scan.
    pub scan_limited: bool,
    /// Relevant results were omitted to respect the result or byte limit.
    pub truncated: bool,
}
