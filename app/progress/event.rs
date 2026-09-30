use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Event {
    #[serde(rename = "event")]
    pub kind: Kind,
    pub phase: Phase,
    pub elapsed_milliseconds: u64,
    pub eta_seconds: Option<u64>,
    pub eta_scope: EtaScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scanned_files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovered_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_messages: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_batches: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_per_second: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_per_second: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stored_vectors: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_vectors: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_inferences: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reused_vectors: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stored_vectors_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) enum Kind {
    #[default]
    #[serde(rename = "index-progress")]
    IndexProgress,
}

impl Event {
    pub fn new(phase: Phase, elapsed_milliseconds: u64, eta_seconds: Option<u64>) -> Self {
        Self {
            phase,
            elapsed_milliseconds,
            eta_seconds,
            eta_scope: EtaScope::Phase,
            ..Self::default()
        }
    }

    pub fn files(
        phase: Phase,
        summary: &crate::ingestion::IndexSummary,
        elapsed_ms: u64,
        eta: Option<u64>,
        scope: EtaScope,
    ) -> Self {
        Self {
            processed_files: Some(summary.processed_files),
            scanned_files: Some(summary.scanned_files),
            processed_bytes: Some(summary.processed_bytes),
            discovered_bytes: Some(summary.discovered_bytes),
            changed_messages: Some(summary.changed_messages),
            committed_batches: Some(summary.committed_batches),
            eta_scope: scope,
            ..Self::new(phase, elapsed_ms, eta)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Phase {
    #[default]
    WaitingForIndex,
    SourceDiscovery,
    Discovered,
    Indexing,
    Checkpoint,
    IngestionComplete,
    SearchRebuild,
    SemanticEmbeddings,
    ModelLoading,
    SemanticFinalize,
    DatabaseCheckpoint,
    LocalComplete,
    SharedSnapshot,
    SharedIngestion,
    SharedEmbeddings,
    SharedUpload,
    SharedCommit,
    SharedPending,
    Complete,
}

impl Phase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::WaitingForIndex => "waiting-for-index",
            Self::SourceDiscovery => "source-discovery",
            Self::Discovered => "discovered",
            Self::Indexing => "indexing",
            Self::Checkpoint => "checkpoint",
            Self::IngestionComplete => "ingestion-complete",
            Self::SearchRebuild => "search-rebuild",
            Self::SemanticEmbeddings => "semantic-embeddings",
            Self::ModelLoading => "model-loading",
            Self::SemanticFinalize => "semantic-finalize",
            Self::DatabaseCheckpoint => "database-checkpoint",
            Self::LocalComplete => "local-complete",
            Self::SharedSnapshot => "shared-snapshot",
            Self::SharedIngestion => "shared-ingestion",
            Self::SharedEmbeddings => "shared-embeddings",
            Self::SharedUpload => "shared-upload",
            Self::SharedCommit => "shared-commit",
            Self::SharedPending => "shared-pending",
            Self::Complete => "complete",
        }
    }
}
impl std::fmt::Display for Phase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EtaScope {
    #[default]
    Phase,
    ProviderIngestion,
    Conversation,
}
impl std::fmt::Display for EtaScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Phase => "phase",
            Self::ProviderIngestion => "provider-ingestion",
            Self::Conversation => "conversation",
        })
    }
}
