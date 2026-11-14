use super::config::Kind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "ServiceState")]
pub(super) enum State {
    Healthy,
    Degraded,
    Stopped,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "ServiceAction")]
pub(crate) enum Action {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub(super) struct Compatibility {
    pub database_schema: Option<i64>,
    pub supported_schema: Option<i64>,
    pub running_build: String,
    pub observer_build: String,
    pub state: State,
    pub blocked: String,
    pub next_step: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub(super) struct Service {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub state: State,
    pub summary: String,
    pub checked_at: u64,
    pub pid: Option<u32>,
    pub actions: Vec<Action>,
    pub control_note: String,
    pub diagnostics: Vec<Diagnostic>,
    pub compatibility: Option<Compatibility>,
    pub logs_available: bool,
    pub control_revision: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub(super) struct Diagnostic {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub(super) struct Machine {
    pub id: String,
    pub platform: String,
    pub checked_at: u64,
    pub reachable: bool,
    pub services: Vec<Service>,
    pub warnings: Vec<String>,
}

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn compatibility(
    database: Option<i64>,
    supported: Option<i64>,
    build: &str,
) -> Compatibility {
    let (state, blocked, next_step) = match (database, supported) {
        (Some(db), Some(version)) if db == version => (
            State::Healthy,
            "None from schema compatibility.",
            "No schema change is needed.",
        ),
        (Some(db), Some(version)) if db > version => (
            State::Failed,
            "This runtime cannot open the newer database schema.",
            "Upgrade the binary, or reconnect the MCP/restart the watcher if a newer binary is already installed. Do not downgrade the database.",
        ),
        (Some(_), Some(_)) => (
            State::Failed,
            "This runtime cannot use the older database until it is upgraded.",
            "Use the matching release's supported SQLite migration path. Unsupported schemas need export/import recovery; restarting alone will not upgrade the database.",
        ),
        _ => (
            State::Unknown,
            "Schema compatibility could not be verified.",
            "Check database access. Reconnect older MCPs that do not report their schema version.",
        ),
    };
    Compatibility {
        database_schema: database,
        supported_schema: supported,
        running_build: build.into(),
        observer_build: crate::BUILD_REVISION.into(),
        state,
        blocked: blocked.into(),
        next_step: next_step.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_compatibility_never_confuses_running_with_usable() {
        assert!(
            compatibility(Some(65), Some(64), "old")
                .blocked
                .contains("newer")
        );
        assert!(
            compatibility(Some(64), Some(65), "new")
                .next_step
                .contains("migration")
        );
        assert_eq!(
            compatibility(Some(65), Some(64), "old").state,
            State::Failed
        );
        assert_eq!(
            compatibility(Some(64), Some(65), "new").state,
            State::Failed
        );
        assert_eq!(
            compatibility(Some(65), Some(65), "old-but-compatible").state,
            State::Healthy
        );
        assert_eq!(
            compatibility(Some(65), None, "legacy").state,
            State::Unknown
        );
        assert_eq!(compatibility(None, Some(65), "new").state, State::Unknown);
    }
}
