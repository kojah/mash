//! Stable typed requests and responses shared by CLI, MCP and search worker.
use crate::config::ConfigurationStatus;
use crate::provider::Provider;
use crate::{semantic, shared};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Paths {
    pub(crate) config: Option<PathBuf>,
    pub(crate) db: Option<PathBuf>,
    pub(crate) models_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) paths: Paths,
    pub(crate) operation: Operation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum IndexMode {
    #[default]
    ProviderFiles,
    LocalGold,
    SharedGold,
}

impl IndexMode {
    pub(crate) fn from_rebuild_flags(local: bool, shared: bool) -> Result<Self, crate::AppError> {
        match (local, shared) {
            (false, false) => Ok(Self::ProviderFiles),
            (true, false) => Ok(Self::LocalGold),
            (false, true) => Ok(Self::SharedGold),
            (true, true) => Err(crate::AppError::usage(
                "local and shared gold rebuilds are mutually exclusive",
            )),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IndexRequest {
    #[serde(default)]
    pub(crate) mode: IndexMode,
    pub(crate) full: bool,
    pub(crate) provider: Vec<Provider>,
    pub(crate) since_days: Option<u32>,
    pub(crate) all_history: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum Operation {
    Index(IndexRequest),
    ListThreads {
        input: Box<crate::thread_catalog::Request>,
        local: bool,
        shared: bool,
    },
    Search {
        #[serde(default)]
        shared_vector_search: Option<crate::gold::vector::Search>,
        query: String,
        #[serde(default)]
        intent: Option<crate::gold::exact::Intent>,
        limit: Option<usize>,
        unit: Option<crate::search_results::SearchUnit>,
        #[serde(default)]
        fusion: crate::gold::config::FusionOverrides,
        #[serde(default)]
        ranking: Option<crate::gold::config::Ranking>,
        #[serde(default)]
        diagnostics: bool,
        filters: Box<crate::search_filter::Selection>,
        shared: bool,
        local: bool,
    },
    View {
        id: String,
        context: u32,
        shared: Option<String>,
    },
    Status {
        shared: bool,
    },
    Forget {
        id: String,
    },
    ModelsInstall,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum Response {
    Index(IndexResponse),
    SharedGoldRebuild(shared::Rebuilt),
    ListThreads(super::threads::Response),
    Search(SearchResponse),
    View(ViewResponse),
    Status(StatusResponse),
    Forget(ForgetResponse),
    ModelsInstall(ModelsInstallResponse),
}

#[derive(Debug, Serialize)]
pub(crate) struct IndexResponse {
    pub(super) gold: Option<crate::gold::manifest::Manifest>,
    pub(super) rebuild_local_gold: bool,
    pub(super) silver: crate::silver::certificate::Certificate,
    pub(super) indexed_conversations: u64,
    pub(super) indexed_messages: u64,
    pub(super) searchable_messages: u64,
    pub(super) scanned_files: u64,
    pub(super) malformed_records: u64,
    pub(super) record_diagnostics: crate::silver::diagnostics::Diagnostics,
    pub(super) changed_messages: u64,
    pub(super) removed_messages: u64,
    pub(super) unchanged_sources: u64,
    pub(super) checkpoint_skipped_sources: u64,
    pub(super) tombstoned_sources: u64,
    pub(super) purged_conversations: u64,
    pub(super) committed_batches: u64,
    pub(super) discovered_bytes: u64,
    pub(super) processed_bytes: u64,
    pub(super) full: bool,
    pub(super) embeddings: u64,
    pub(super) model_inferences: u64,
    pub(super) reused_embeddings: u64,
    pub(super) realized_mode: SearchMode,
    pub(super) model_load_milliseconds: u64,
    pub(super) storage_setup_milliseconds: u64,
    pub(super) ingestion_milliseconds: u64,
    pub(super) search_refresh_milliseconds: u64,
    pub(super) embedding_milliseconds: u64,
    pub(super) total_milliseconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) shared: Option<shared::Publication>,
}

impl Response {
    pub(crate) fn needs_retry(&self) -> bool {
        matches!(self, Self::Index(result) if result.needs_retry())
    }
}

impl IndexResponse {
    pub(super) fn needs_retry(&self) -> bool {
        !self.silver.is_complete()
            || self.malformed_records > 0
            || self
                .shared
                .as_ref()
                .is_some_and(shared::Publication::is_pending)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SearchMode {
    Lexical,
    LocalLexical,
    MergedLexical,
    Hybrid,
    LocalHybrid,
    MergedHybrid,
    Unavailable,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SearchResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostics: Option<crate::gold::diagnostics::Diagnostics>,
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) intent: crate::gold::exact::Intent,
    pub(crate) realized_mode: SearchMode,
    pub(crate) unit: crate::search_results::SearchUnit,
    pub(crate) results: crate::search_results::Results,
    pub(crate) shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shared_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) shared_context_error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ViewResponse {
    pub(crate) id: String,
    pub(crate) messages: Vec<crate::storage::Message>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusResponse {
    pub(super) lexical: Option<Box<crate::gold::lexical_manifest::Manifest>>,
    pub(super) gold: Option<crate::gold::manifest::Manifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) silver: Option<crate::silver::certificate::Certificate>,
    pub(super) ready: bool,
    pub(super) database_path: Option<PathBuf>,
    pub(super) conversations: u64,
    pub(super) messages: u64,
    pub(super) searchable_messages: u64,
    pub(super) embeddings: u64,
    pub(super) stored_embeddings: u64,
    pub(super) models_installed: bool,
    pub(super) query_models: ModelAvailability,
    pub(super) semantic_support: bool,
    pub(super) realized_mode: SearchMode,
    pub(super) recommended_action: Option<crate::Action>,
    pub(super) configuration: ConfigurationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) shared: Option<shared::Publication>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) shared_index: Option<shared::Freshness>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ModelAvailability {
    NotRequired,
    Available,
    Unavailable,
}

#[derive(Debug, Serialize)]
pub(crate) struct ForgetResponse {
    pub(super) id: String,
    pub(super) forgotten: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) shared: Option<shared::Publication>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelsInstallResponse {
    pub(super) installed: bool,
    pub(super) model_directory: PathBuf,
    #[serde(flatten)]
    pub(super) summary: semantic::InstallSummary,
    pub(super) recommended_action: crate::Action,
}
