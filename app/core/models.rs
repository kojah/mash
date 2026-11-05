//! Stable typed requests and responses shared by CLI and MCP.
use crate::config::ConfigurationStatus;
use crate::provider::Provider;
pub(super) use crate::views::readiness::{Artifacts as LocalArtifacts, State as ArtifactState};
use crate::{semantic, shared};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub(crate) struct Paths {
    pub(crate) config: Option<PathBuf>,
    pub(crate) db: Option<PathBuf>,
    pub(crate) models_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct Request {
    pub(crate) paths: Paths,
    pub(crate) operation: Operation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum IndexMode {
    #[default]
    ProviderFiles,
    LocalViews,
}

impl IndexMode {
    pub(crate) const fn from_rebuild_flag(local_views: bool) -> Self {
        if local_views {
            Self::LocalViews
        } else {
            Self::ProviderFiles
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct IndexRequest {
    pub(crate) mode: IndexMode,
    pub(crate) full: bool,
    pub(crate) provider: Vec<Provider>,
    pub(crate) since_days: Option<u32>,
    pub(crate) all_history: bool,
    /// Watch passes queue publication instead of awaiting it, so a slow
    /// network publisher never occupies the local indexing task.
    pub(crate) defer_publication: bool,
    /// Publish as a progressive baseline instead of a replacing snapshot.
    pub(crate) backfill: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum Operation {
    /// Configure, register with agents and index for the first time.
    Init(super::setup::InitRequest),
    Index(IndexRequest),
    IngestConnector {
        batch_file: PathBuf,
    },
    /// Deliver a connector batch to the configured shared destination.
    DeliverConnector {
        batch_file: PathBuf,
    },
    ListChats {
        input: Box<crate::chat_catalog::Request>,
        local: bool,
        shared: bool,
        diagnostics: bool,
    },
    Search {
        shared_vector_search: Option<crate::views::vector::Search>,
        query: String,
        intent: Option<crate::views::exact::Intent>,
        limit: Option<usize>,
        offset: usize,
        unit: Option<crate::search_results::SearchUnit>,
        fusion: crate::views::config::FusionOverrides,
        ranking: Option<crate::views::config::Ranking>,
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
        coverage: Option<super::coverage::Scope>,
    },
    Installation {
        agents: Vec<super::installation::Agent>,
        probe: bool,
    },
    Forget {
        id: String,
    },
    /// One read-only SQL statement over local silver.
    QuerySilver {
        sql: String,
        limit: usize,
        timeout_seconds: u64,
    },
    /// A typed, declarative query over the stable views.
    QueryStructured {
        request: crate::silver_query::Request,
        timeout_seconds: u64,
    },
    /// The same request over the configured shared index, scoped by the
    /// bearer token's group; `node` defaults to this node.
    QueryShared {
        request: crate::silver_query::Request,
        node: Option<String>,
    },
    ModelsInstall,
    ExportSilver {
        directory: PathBuf,
    },
    ImportSilver {
        directory: PathBuf,
        database: PathBuf,
        keep_identity: bool,
    },
    ListAttachments {
        message_id: String,
        shared: Option<String>,
    },
    GetAttachment {
        message_id: String,
        attachment_id: String,
        shared: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum Response {
    Init(super::setup::InitResponse),
    Index(IndexResponse),
    IngestConnector(crate::connectors::Receipt),
    ListChats(super::chats::Response),
    Search(SearchResponse),
    View(ViewResponse),
    Status(Box<StatusResponse>),
    Installation(super::installation::Report),
    Forget(ForgetResponse),
    Query(serde_json::Value),
    ModelsInstall(ModelsInstallResponse),
    ExportSilver(crate::export::Exported),
    ImportSilver(crate::export::Imported),
    ListAttachments(crate::attachments::Listing),
    GetAttachment(crate::attachments::Retrieved),
}

#[derive(Debug, Serialize)]
pub(crate) struct IndexResponse {
    pub(super) views: Option<crate::views::manifest::Manifest>,
    pub(super) rebuild_local_views: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) backfill: Option<BackfillSummary>,
}

/// What a `--backfill` publication produced.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct BackfillSummary {
    pub(crate) id: String,
    pub(crate) revision: i64,
}

impl Response {
    /// An installation report where an agent named with `--agent` needs attention.
    pub(crate) fn required_agent_needs_attention(&self) -> bool {
        matches!(self, Self::Installation(report) if report.required_attention())
    }

    pub(crate) fn needs_retry(&self) -> bool {
        match self {
            Self::Index(result) => result.needs_retry(),
            Self::Init(result) => result
                .index
                .as_ref()
                .is_some_and(|index| index.needs_retry()),
            _ => false,
        }
    }
}

impl IndexResponse {
    /// A retry can help only incomplete silver (a rejected or unfinished
    /// source) or a pending publication. Records skipped from a source whose
    /// rest was applied are permanent damage, reported but never retried.
    pub(super) fn needs_retry(&self) -> bool {
        !self.silver.is_complete()
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
    pub(crate) diagnostics: Option<crate::views::diagnostics::Diagnostics>,
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) intent: crate::views::exact::Intent,
    pub(crate) realized_mode: SearchMode,
    pub(crate) unit: crate::search_results::SearchUnit,
    pub(crate) results: crate::search_results::Results,
    /// Offset of the next page of the same ranked list; null on the last page.
    #[serde(default)]
    pub(crate) next_offset: Option<usize>,
    pub(crate) shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shared_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) shared_context_error: Option<String>,
    /// Why an empty result may not mean the history lacks the subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) notice: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ViewResponse {
    pub(crate) id: String,
    pub(crate) messages: Vec<crate::storage::Message>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) local_views_error: Option<String>,
    pub(super) installation: super::installation::Summary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) coverage: Option<super::coverage::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) shared_error: Option<String>,
    pub(super) local_artifacts: LocalArtifacts,
    pub(super) lexical: Option<crate::views::lexical_manifest::Manifest>,
    pub(super) views: Option<crate::views::manifest::Manifest>,
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
    /// Each configured provider and connector source, with its sync modes.
    pub(super) sources: Vec<crate::source::SourceStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) shared: Option<shared::Publication>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) shared_index: Option<shared::Freshness>,
    /// Duplicate-history report, only when `--shared` asked for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) duplicates: Option<shared::FingerprintReport>,
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
    pub(super) local_views_error: Option<String>,
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
