//! Typed interpretations of native chat history, distinct from source messages.
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::AppError;
pub(crate) mod matches;
pub(crate) mod recall;
pub(crate) mod transfer;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Scope {
    Personal,
    Repository { identity: String },
    LocalProject { path: String },
    Chat { id: String },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Review {
    #[default]
    Candidate,
    Established,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecisionState {
    Proposed,
    Accepted,
    Superseded,
    Withdrawn,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreferenceState {
    Active,
    Superseded,
    Withdrawn,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FactState {
    Current,
    Superseded,
    Retracted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QuestionState {
    Open,
    Resolved,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Details {
    Decision {
        state: DecisionState,
        #[serde(default)]
        alternatives: Vec<String>,
        #[serde(default)]
        consequences: Vec<String>,
    },
    Preference {
        state: PreferenceState,
        #[serde(default)]
        conditions: Vec<String>,
    },
    Fact {
        state: FactState,
        reported_by: String,
        valid_until: Option<i64>,
    },
    OpenQuestion {
        state: QuestionState,
        answer: Option<String>,
    },
}

impl Details {
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Decision { .. } => "decision",
            Self::Preference { .. } => "preference",
            Self::Fact { .. } => "fact",
            Self::OpenQuestion { .. } => "open_question",
        }
    }

    pub(crate) const fn current(&self) -> bool {
        matches!(
            self,
            Self::Decision {
                state: DecisionState::Accepted,
                ..
            } | Self::Preference {
                state: PreferenceState::Active,
                ..
            } | Self::Fact {
                state: FactState::Current,
                ..
            } | Self::OpenQuestion {
                state: QuestionState::Open,
                ..
            }
        )
    }

    pub(crate) fn supersede(&mut self) -> Result<(), AppError> {
        if !self.current() {
            return Err(conflict("only current records can be superseded"));
        }
        match self {
            Self::Decision { state, .. } => *state = DecisionState::Superseded,
            Self::Preference { state, .. } => *state = PreferenceState::Superseded,
            Self::Fact { state, .. } => *state = FactState::Superseded,
            Self::OpenQuestion { .. } => {
                return Err(AppError::usage("resolve or drop an open question"));
            }
        }
        Ok(())
    }

    pub(crate) const fn superseded(&self) -> bool {
        matches!(
            self,
            Self::Decision {
                state: DecisionState::Superseded,
                ..
            } | Self::Preference {
                state: PreferenceState::Superseded,
                ..
            } | Self::Fact {
                state: FactState::Superseded,
                ..
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Body {
    pub statement: String,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub review: Review,
    pub details: Details,
    /// When the choice/claim applied, in Unix milliseconds; unknown stays null.
    pub effective_at: Option<i64>,
    /// Attribution supplied by the caller, not authenticated human identity.
    pub recorded_by: String,
}

impl Body {
    pub(crate) fn applies_at(&self, timestamp: i64) -> bool {
        self.details.current()
            && self.effective_at.is_none_or(|start| start <= timestamp)
            && !matches!(&self.details, Details::Fact { valid_until: Some(end), .. } if *end < timestamp)
    }

    pub(crate) fn validate(&self) -> Result<(), AppError> {
        bounded(&self.statement, 4096, "statement")?;
        bounded(&self.recorded_by, 256, "recorded_by")?;
        if self.rationale.len() > 8192 {
            return Err(AppError::usage("rationale exceeds 8192 bytes"));
        }
        if let Details::Fact {
            reported_by,
            valid_until,
            ..
        } = &self.details
        {
            bounded(reported_by, 256, "reported_by")?;
            if valid_until
                .zip(self.effective_at)
                .is_some_and(|(end, start)| end < start)
            {
                return Err(AppError::usage("fact validity ends before it starts"));
            }
        }
        if let Details::OpenQuestion { state, answer } = &self.details {
            if *state == QuestionState::Resolved
                && answer.as_ref().is_none_or(|text| text.trim().is_empty())
            {
                return Err(AppError::usage("resolved questions require an answer"));
            }
            if *state == QuestionState::Open && answer.is_some() {
                return Err(AppError::usage(
                    "open questions cannot carry a resolved answer",
                ));
            }
        }
        let encoded = encode(self)?;
        if encoded.len() > 16384 {
            return Err(AppError::usage("record body exceeds 16384 bytes"));
        }
        if crate::redaction::private(&encoded) != encoded {
            return Err(AppError::usage(
                "record text must not contain private spans",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceUse {
    #[default]
    Context,
    /// Caller asserts this is the person's endorsement, not an injected report.
    Endorsement,
    Report,
}

impl rusqlite::types::ToSql for EvidenceUse {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(match self {
            Self::Context => "context",
            Self::Endorsement => "endorsement",
            Self::Report => "report",
        }
        .into())
    }
}

impl rusqlite::types::FromSql for EvidenceUse {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match value.as_str()? {
            "context" => Ok(Self::Context),
            "endorsement" => Ok(Self::Endorsement),
            "report" => Ok(Self::Report),
            _ => Err(rusqlite::types::FromSqlError::InvalidType),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Citation {
    pub message_id: String,
    pub quote: String,
    #[serde(default)]
    pub purpose: EvidenceUse,
    pub expected_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expected {
    pub id: String,
    pub revision: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DuplicatePolicy {
    #[default]
    Report,
    Reuse,
}

impl DuplicatePolicy {
    pub(crate) const fn is_report(&self) -> bool {
        matches!(self, Self::Report)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Change {
    Create {
        scope: Scope,
        body: Body,
        evidence: Vec<Citation>,
        #[serde(default, skip_serializing_if = "DuplicatePolicy::is_report")]
        on_duplicate: DuplicatePolicy,
    },
    Revise {
        expected: Expected,
        body: Body,
        evidence: Vec<Citation>,
    },
    Supersede {
        expected: Expected,
        body: Body,
        evidence: Vec<Citation>,
    },
    Confirm {
        expected: Expected,
        evidence: Vec<Citation>,
    },
    Resolve {
        expected: Expected,
        answer: String,
        /// Optional current record that answers the question, checked atomically.
        resolved_by: Option<Expected>,
        evidence: Vec<Citation>,
    },
    Forget {
        expected: Expected,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mutation {
    pub operation_id: String,
    pub change: Change,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub record_id: String,
    pub revision: i64,
    pub related: Option<Expected>,
    #[serde(default)]
    pub reused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Evidence {
    pub chat_id: String,
    pub message_id: String,
    pub fingerprint: String,
    pub quote: String,
    /// Unknown for records written before evidence provenance was retained.
    #[serde(default)]
    pub purpose: Option<EvidenceUse>,
    #[serde(default)]
    pub source_role: Option<crate::role::Role>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecordRevision {
    pub revision: i64,
    pub body: Body,
    pub recorded_at: i64,
    pub evidence: Vec<Evidence>,
    pub evidence_current: bool,
    pub confirmations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Record {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub id: String,
    pub owner: String,
    pub scope: Scope,
    pub current_revision: i64,
    pub revisions: Vec<RecordRevision>,
    pub links: Vec<Link>,
}

impl Record {
    pub(crate) fn redact(&mut self, policy: crate::redaction::Policy) {
        match &mut self.scope {
            Scope::Repository { identity } => *identity = policy.text(identity),
            Scope::LocalProject { path } => *path = policy.text(path),
            Scope::Personal | Scope::Chat { .. } => {}
        }
        for revision in &mut self.revisions {
            revision.body.redact(policy);
            for source in &mut revision.evidence {
                source.quote = policy.text(&source.quote);
            }
        }
    }
}

impl Body {
    pub(crate) fn redact(&mut self, policy: crate::redaction::Policy) {
        let body = self;
        body.statement = policy.text(&body.statement);
        body.rationale = policy.text(&body.rationale);
        body.recorded_by = policy.text(&body.recorded_by);
        match &mut body.details {
            Details::Decision {
                alternatives,
                consequences,
                ..
            } => {
                for text in alternatives.iter_mut().chain(consequences.iter_mut()) {
                    *text = policy.text(text);
                }
            }
            Details::Preference { conditions, .. } => {
                for text in conditions {
                    *text = policy.text(text);
                }
            }
            Details::Fact { reported_by, .. } => *reported_by = policy.text(reported_by),
            Details::OpenQuestion { answer, .. } => {
                if let Some(text) = answer {
                    *text = policy.text(text);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Link {
    pub source_id: String,
    pub target_id: String,
    pub relation: Relation,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Relation {
    Supersedes,
    Resolves,
    Supports,
    Contradicts,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct List {
    #[serde(default)]
    pub shared: bool,
    pub node_id: Option<String>,
    pub cursor: Option<Cursor>,
    pub scope: Option<Scope>,
    pub query: Option<String>,
    #[serde(default)]
    pub include_inactive: bool,
    #[serde(default)]
    #[schemars(range(max = 100_000))]
    pub offset: usize,
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Page {
    pub records: Vec<Record>,
    pub next_offset: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    pub node_id: String,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Get {
    pub id: String,
    #[serde(default)]
    pub history: bool,
    pub node_id: Option<String>,
}

pub(crate) fn bounded(value: &str, max: usize, field: &str) -> Result<(), AppError> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(AppError::usage(format!(
            "{field} must contain 1 to {max} bytes and no NUL"
        )));
    }
    Ok(())
}

pub(crate) fn conflict(message: &str) -> AppError {
    AppError::new(12, crate::ErrorKind::Conflict, message, false)
}

pub(crate) fn encode(value: &impl Serialize) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|_| AppError::internal("serialize native record"))
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(|_| AppError::database_data("invalid native record data"))
}
