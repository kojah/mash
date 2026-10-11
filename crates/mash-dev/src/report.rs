use rmcp::model::{CallToolResult, ProgressNotificationParam};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Running,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Report {
    pub state: State,
    #[serde(default)]
    pub progress: Option<ProgressNotificationParam>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub results: Vec<CallToolResult>,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            state: State::Running,
            progress: None,
            error: None,
            results: vec![],
        }
    }
}
