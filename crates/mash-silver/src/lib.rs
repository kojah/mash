//! Silver's provider-independent chat model and the native history adapters
//! that produce it: discovery, parsing and reconciliation inputs. Writing
//! silver to SQLite is `mash`'s job; nothing here opens the index.
//!
//! An internal crate: its public items are what the rest of the workspace may
//! use, not a supported API.
use std::path::Path;
use std::time::Duration;

pub use mash_model::diagnostics;
pub use mash_model::progress::IndexSummary;
use mash_model::{
    AppError, ErrorKind, attachments, message_context, platform, progress, provider, redaction,
    role, scratch, tools,
};
use provider::Provider;

pub mod admission;
pub mod antigravity;
pub mod certificate;
pub mod claude;
pub mod codex_store;
pub mod copilot;
pub mod cursor;
pub mod dialogue;
pub mod discovery;
pub mod enrichment;
pub mod identities;
pub mod json;
pub mod messages;
pub mod native;
pub mod opencode;
pub mod parsers;
pub mod prefix;
pub mod records;
pub mod revision;
pub mod source;
pub mod sqlite_source;
pub mod turns;

pub use discovery::{SessionContent, SourceStamp, source_stamp_for};
#[cfg(any(test, feature = "test-support"))]
pub use discovery::{discovered_sources, parse_test_source, source_stamp};
pub use discovery::{validate_output_path, watch_roots};
pub use source::{HistorySource, SourceIdentity};

pub const MAX_TOOL_OUTPUT_CHARS: usize = 128 * 1024;
pub const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub enum Parser {
    Native(Provider),
    #[cfg(any(test, feature = "test-support"))]
    Custom(fn(&HistorySource) -> Result<ParsedFile, AppError>),
}

impl Parser {
    pub fn parse(self, source: &HistorySource) -> Result<ParsedFile, AppError> {
        let provider = match self {
            Self::Native(provider) => provider,
            #[cfg(any(test, feature = "test-support"))]
            Self::Custom(parse) => return parse(source),
        };
        match provider {
            Provider::ClaudeCode => parsers::parse_claude(&source.identity.path),
            Provider::Codex if source.identity.session.is_some() => {
                codex_store::parse(&source.identity)
            }
            Provider::Codex => parsers::parse_codex(&source.identity.path),
            Provider::Opencode => opencode::parse(&source.identity),
            Provider::Copilot => copilot::parse_source(source),
            Provider::Cursor => cursor::parse(&source.identity),
            Provider::Antigravity => antigravity::parse(&source.identity),
        }
    }
}

#[derive(Clone)]
pub struct IndexOptions {
    pub providers: crate::provider::ProviderRoots,
    pub roots_are_authoritative: bool,
    pub since_days: Option<u32>,
}

pub struct Chat {
    pub id: String,
    pub provider: Provider,
    pub source: SourceIdentity,
    pub claude_lineage: Option<claude::Lineage>,
    /// Adapter-selected title provenance before native/fallback choices flatten.
    pub title_origin: Option<enrichment::Origin>,
    /// Adapter-selected timestamp provenance before fallback choices flatten.
    pub timestamp_origins: Option<enrichment::Timestamps>,
    pub project: Option<String>,
    pub repository: Option<String>,
    pub title: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub messages: messages::Messages,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedMessage {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::attachments::Source>,
    pub id: String,
    pub ordinal: i64,
    pub context: crate::message_context::MessageContext,
    pub role: crate::role::Role,
    pub content: String,
    /// Silver's interpreted visible dialogue. `None` uses canonical text;
    /// an empty override is context-only. Native hidden turns, tool blocks and
    /// injected context are interpreted here, independently of retrieval policy.
    pub dialogue_text: Option<String>,
    pub created_at: Option<i64>,
    /// Structured tool calls and results; their text stays in `content`.
    #[serde(default, skip_serializing_if = "crate::tools::Activity::is_empty")]
    pub tools: crate::tools::Activity,
}

impl NormalizedMessage {
    /// Masks `<private>` spans, copying the message only when it holds one.
    pub fn mask_private(message: &mut std::borrow::Cow<'_, Self>) {
        use crate::redaction::{private, private_text};
        let tagged = |text: &str| text.to_ascii_lowercase().contains("<private>");
        let tools = message.tools.encode().ok().flatten();
        if !(tagged(&message.content)
            || message.dialogue_text.as_deref().is_some_and(tagged)
            || tools.as_deref().is_some_and(tagged))
        {
            return;
        }
        let message = message.to_mut();
        private_text(&mut message.content);
        if let Some(text) = &mut message.dialogue_text {
            private_text(text);
        }
        message
            .tools
            .map_strings(|value| private(value).into_owned());
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn dialogue(&self) -> &str {
        self.dialogue_text.as_deref().unwrap_or(&self.content)
    }
}

pub struct ParsedFile {
    pub chat: Option<Chat>,
    pub malformed_records: u64,
    pub first_record_issue: Option<diagnostics::RecordIssue>,
}

/// Parse one provider source as it is now, without writing silver.
/// `None` means the snapshot ended in an incomplete record.
pub fn read_transcript(
    provider: Provider,
    source: &HistorySource,
) -> Result<Option<Chat>, AppError> {
    let mut stamp = source.stamp()?;
    Ok(parse_source_with_retries(source, &mut stamp, Parser::Native(provider))?.chat)
}

/// A Codex session's source from the thread store at `store`, as indexing
/// selects it, so message identities match what indexing will retain.
pub fn codex_session_source(
    store: &Path,
    session: &str,
) -> Result<Option<HistorySource>, AppError> {
    codex_store::session_source(store, session)
}

/// How many times one source is read before it is reported as still changing.
pub const SOURCE_ATTEMPTS: u64 = 8;

#[tracing::instrument(name = "parse_source", skip_all, fields(attempts = tracing::field::Empty))]
pub fn parse_source_with_retries(
    source: &HistorySource,
    stamp: &mut SourceStamp,
    parse: Parser,
) -> Result<ParsedFile, AppError> {
    // Discovery can precede a source's turn in the bounded parser queue by
    // minutes. Capture a fresh stamp for each attempt and verify the consumed
    // snapshot. Never retry malformed input or other parser errors. An agent
    // that is mid-conversation commits every few tens of milliseconds, so each
    // attempt can lose that race; eight make losing them all rare.
    for attempt in 0..SOURCE_ATTEMPTS {
        tracing::Span::current().record("attempts", attempt + 1);
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(50));
        }
        *stamp = match source.stamp() {
            Ok(stamp) => stamp,
            Err(error) if error.error.kind == ErrorKind::SourceChanged => continue,
            Err(error) => return Err(error),
        };
        match parse_stable_source(source, stamp, parse) {
            Err(error) if error.error.kind == ErrorKind::SourceChanged => {}
            result => return result,
        }
    }
    Err(AppError::source_changed())
}

pub fn parse_stable_source(
    source: &HistorySource,
    stamp: &mut SourceStamp,
    parse: Parser,
) -> Result<ParsedFile, AppError> {
    stamp.verify(&source.identity)?;
    let parsed = if let Parser::Native(provider @ (Provider::ClaudeCode | Provider::Codex)) = parse
        && source.identity.session.is_none()
        && source.dependencies.is_empty()
    {
        let (file, proof) = prefix::capture(&source.identity.path, stamp)?;
        stamp.prefix = Some(proof);
        parsers::parse_captured(&source.identity.path, provider, file)
    } else {
        parse.parse(source)
    };
    stamp.verify(&source.identity)?;
    parsed
}
