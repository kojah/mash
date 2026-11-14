use super::{Body, Citation, DuplicatePolicy, Receipt, Scope};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    pub scope: Scope,
    pub chat_id: String,
}

#[derive(Debug, Serialize)]
pub struct State {
    pub source_fingerprint: String,
    pub processed_fingerprint: Option<String>,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub body: Body,
    pub evidence: Vec<Citation>,
    #[serde(default, skip_serializing_if = "DuplicatePolicy::is_report")]
    pub on_duplicate: DuplicatePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation_id: String,
    pub scope: Scope,
    pub chat_id: String,
    pub source_fingerprint: String,
    pub records: Vec<Draft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Result {
    pub source_fingerprint: String,
    pub records: Vec<Receipt>,
}
