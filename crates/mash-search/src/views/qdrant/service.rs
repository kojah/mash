//! How a local Qdrant installed by `mash qdrant install` is supervised, as status reports it.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Supervisor {
    SystemdUser,
    LaunchAgent,
    ScheduledTask,
}

/// What the supervisor reports about the installed service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    /// The supervisor has no such service.
    NotInstalled,
    Running,
    Starting,
    Stopped,
    /// The supervisor gave up after it exited with an error.
    Failed,
    /// The supervisor could not be asked.
    Unknown,
}

impl ServiceState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Starting)
    }
}

/// A Qdrant installed by `mash qdrant install`, as `mash status` reports it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Managed {
    pub version: String,
    pub state: ServiceState,
    pub supervisor: Supervisor,
    /// The systemd unit, launchd label or scheduled task name.
    pub service: String,
    pub definition: PathBuf,
    pub binary: PathBuf,
    pub storage: PathBuf,
    /// Where Qdrant's own output goes: a file, or the command that shows it.
    pub logs: String,
}
