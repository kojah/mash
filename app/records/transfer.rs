//! Portable canonical record rows. Computed freshness and search scores are absent.
use super::{Body, Evidence, Link, Receipt, Scope};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Row {
    Owner {
        id: String,
    },
    Header {
        id: String,
        owner: String,
        kind: Kind,
        scope: Option<Scope>,
        revision: i64,
        forgotten: bool,
    },
    Revision {
        id: String,
        revision: i64,
        body: Body,
        recorded_at: i64,
    },
    Evidence {
        id: String,
        revision: i64,
        position: i64,
        source: Evidence,
    },
    Link {
        link: Link,
    },
    Confirmation {
        id: String,
        revision: i64,
        message_id: String,
        confirmed_at: i64,
    },
    Operation {
        id: String,
        digest: String,
        result: Receipt,
    },
    Checkpoint {
        scope: Scope,
        chat_id: String,
        source_fingerprint: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Decision,
    Preference,
    Fact,
    OpenQuestion,
}

impl Kind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Preference => "preference",
            Self::Fact => "fact",
            Self::OpenQuestion => "open_question",
        }
    }
}
