//! The typed error every Mash operation reports, as JSON and as an exit code.
use std::io;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct AppError {
    pub error: ErrorBody,
    #[serde(skip)]
    pub code: i32,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ErrorBody {
    pub kind: ErrorKind,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<crate::database_error::DatabaseDetails>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    SharedIndex,
    QdrantUnavailable,
    QdrantFailure,
    Configuration,
    Usage,
    DatabaseMissing,
    IndexBusy,
    ModelBusy,
    Database,
    Io,
    Internal,
    Model,
    SearchNotReady,
    SchemaIncompatible,
    RemoteInference,
    Conflict,
    SourceChanged,
    SearchTimeout,
    NotFound,
    QdrantInstall,
}

impl ErrorKind {
    /// The kebab-case name used in JSON errors and on the sync wire.
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    pub fn parse(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[schemars(rename = "RecommendedAction")]
pub enum Action {
    #[serde(rename = "init")]
    Init,
    #[serde(rename = "index")]
    Index,
    #[serde(rename = "models install")]
    ModelsInstall,
    /// Not a Mash command: start the Qdrant service at the reported URL.
    #[serde(rename = "start-qdrant")]
    StartQdrant,
}

impl AppError {
    pub fn source_changed() -> Self {
        Self::with_action(
            9,
            ErrorKind::SourceChanged,
            "history changed while indexing; retry index after the source settles",
            true,
            Action::Index,
        )
    }

    pub fn shared(message: impl Into<String>) -> Self {
        Self::new(11, ErrorKind::SharedIndex, message, true)
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(9, ErrorKind::Configuration, message, false)
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(2, ErrorKind::Usage, message, false)
    }

    /// Nothing is configured or indexed yet: setup, not models, comes first.
    pub fn not_set_up(path: &std::path::Path) -> Self {
        Self::with_action(
            3,
            ErrorKind::DatabaseMissing,
            format!(
                "Mash is not set up on this machine (no configuration, and no index at {}); run `mash init`",
                path.display()
            ),
            false,
            Action::Init,
        )
    }

    /// Configured, but no index yet: `mash index` comes next, not `init`.
    pub fn not_indexed(path: &std::path::Path, qdrant: &str) -> Self {
        Self::with_action(
            3,
            ErrorKind::DatabaseMissing,
            format!(
                "Mash is configured, but nothing is indexed yet (no index at {}). Run `mash index`: it saves your history, which `mash list-chats` and `mash view` read, then builds search, which needs Qdrant at {qdrant} (`mash status` checks it).",
                path.display()
            ),
            true,
            Action::Index,
        )
    }

    pub fn missing_database(path: &std::path::Path) -> Self {
        Self::with_action(
            3,
            ErrorKind::DatabaseMissing,
            format!(
                "database does not exist: {}; run `mash init` to set up Mash, or `mash index` if it is already configured",
                path.display()
            ),
            true,
            Action::Index,
        )
    }

    pub fn database(error: rusqlite::Error) -> Self {
        crate::database_error::error(crate::database_error::Operation::Unspecified, error)
    }

    pub fn database_at(
        operation: crate::database_error::Operation,
    ) -> impl FnOnce(rusqlite::Error) -> Self {
        move |error| crate::database_error::error(operation, error)
    }

    pub fn database_data(message: impl Into<String>) -> Self {
        Self::new(5, ErrorKind::Database, message, false)
    }

    pub fn io(error: io::Error) -> Self {
        let message = error.to_string();
        drop(error);
        Self::new(5, ErrorKind::Io, message, true)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(10, ErrorKind::Internal, message, false)
    }

    pub fn model(message: impl Into<String>) -> Self {
        Self::with_action(4, ErrorKind::Model, message, true, Action::ModelsInstall)
    }

    /// Models could not be fetched because their host is unreachable.
    pub fn model_unreachable(message: impl Into<String>) -> Self {
        Self::new(4, ErrorKind::Model, message, false)
    }

    /// Semantic search was requested without models. A new user usually got
    /// here by indexing before `mash init`, which sets up search without them.
    pub fn models_missing() -> Self {
        Self::model(
            "semantic models are not installed; run `mash models install`, or on a new machine `mash init` to set up word-exact search without them",
        )
    }

    pub fn model_busy() -> Self {
        Self::with_action(
            6,
            ErrorKind::ModelBusy,
            "another model installer is active; retry after it finishes",
            true,
            Action::ModelsInstall,
        )
    }

    pub fn search_timeout(limit: std::time::Duration) -> Self {
        Self::new(
            12,
            ErrorKind::SearchTimeout,
            format!(
                "search did not finish within {} s and was stopped; retry, or narrow it with filters",
                limit.as_secs()
            ),
            true,
        )
    }

    /// A requested ID names nothing in the index that was read.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(13, ErrorKind::NotFound, message, false)
    }

    pub fn search_not_ready(message: impl Into<String>) -> Self {
        Self::with_action(8, ErrorKind::SearchNotReady, message, true, Action::Index)
    }

    /// Search is behind only because an index pass is still running. Trying
    /// again is the remedy; starting another index would collide with it.
    pub fn search_catching_up() -> Self {
        Self::new(
            8,
            ErrorKind::SearchNotReady,
            "search is catching up with new history; try again in a moment",
            true,
        )
    }

    /// Search is behind the stored history and nothing is refreshing it.
    pub fn search_behind() -> Self {
        Self::search_not_ready(
            "search is behind the stored history and nothing is refreshing it; run `mash index`",
        )
    }

    pub fn schema(message: impl Into<String>) -> Self {
        Self::new(7, ErrorKind::SchemaIncompatible, message, false)
    }

    pub fn new(code: i32, kind: ErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            error: ErrorBody {
                kind,
                message: message.into(),
                retryable,
                recommended_action: None,
                database: None,
            },
            code,
        }
    }

    pub fn with_action(
        code: i32,
        kind: ErrorKind,
        message: impl Into<String>,
        retryable: bool,
        recommended_action: Action,
    ) -> Self {
        let mut error = Self::new(code, kind, message, retryable);
        error.error.recommended_action = Some(recommended_action);
        error
    }
}
