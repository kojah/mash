//! Automatic agent-assisted capture policy and bounded review discovery.
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
    /// Fixed setup cutoff for explicit backfill; absent uses the 3-day default.
    pub backfill_since: Option<i64>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            backfill_since: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Request {
    pub session_id: Option<String>,
    /// Explicit setup/maintenance only. Ordinary conversations never scan history.
    pub backfill: bool,
    /// Continue a bounded scan; offsets are hints, not review checkpoints.
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub chat_id: String,
    pub scope: super::Scope,
    pub source_fingerprint: String,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub enabled: bool,
    pub backfill_since: Option<i64>,
    pub current: Option<Task>,
    /// At most one unreviewed historical chat per request.
    pub backfill: Option<Task>,
    pub next_offset: Option<usize>,
    pub scan_limited: bool,
}
