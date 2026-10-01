use crate::AppError;
use rusqlite::Row;
use serde::Serialize;

#[derive(Clone, PartialEq, Eq, Serialize)]
pub(in crate::topics::evaluation) struct Identity {
    message_id: String,
    ordinal: i64,
    canonical_hash: [u8; 32],
}

impl Identity {
    fn valid(&self) -> bool {
        !self.message_id.is_empty() && self.message_id.len() <= 256 && self.ordinal >= 0
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(in crate::topics::evaluation) enum Origin {
    Stored {
        message_id: String,
        ordinal: i64,
        canonical_hash: [u8; 32],
    },
    Contextual {
        user: Identity,
        assistant: Identity,
        user_text_chars: usize,
        user_derived_hash: [u8; 32],
        assistant_text_kind: TextKind,
        assistant_prefix_chars: usize,
        assistant_prefix_hash: [u8; 32],
        assistant_prefix_complete: bool,
    },
    Synthetic,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::topics::evaluation) enum TextKind {
    Canonical,
    SearchProjection,
    TopicProjection,
    ContextualRequest,
    Synthetic,
}

#[derive(Clone)]
pub(in crate::topics::evaluation) struct Source {
    pub(in crate::topics::evaluation) text: String,
    pub(super) origin: Origin,
    pub(super) kind: TextKind,
}

impl Source {
    pub(in crate::topics::evaluation) fn reviewed_canonical(
        &self,
        message: &str,
        hash: &[u8; 32],
    ) -> bool {
        matches!(self.kind, TextKind::Canonical)
            && matches!(&self.origin, Origin::Stored { message_id, canonical_hash, .. }
                if message_id == message && canonical_hash == hash)
            && blake3::hash(self.text.as_bytes()).as_bytes() == hash
    }

    fn identity(&self) -> Option<Identity> {
        let Origin::Stored {
            message_id,
            ordinal,
            canonical_hash,
        } = &self.origin
        else {
            return None;
        };
        Some(Identity {
            message_id: message_id.clone(),
            ordinal: *ordinal,
            canonical_hash: *canonical_hash,
        })
    }

    pub(in crate::topics::evaluation) fn anchor(&self) -> Option<(&str, i64)> {
        match &self.origin {
            Origin::Stored {
                message_id,
                ordinal,
                ..
            } => Some((message_id, *ordinal)),
            _ => None,
        }
    }

    pub(in crate::topics::evaluation) fn with_context(
        &self,
        anchor: &Self,
        answer: &Self,
        answer_complete: bool,
    ) -> Result<Self, AppError> {
        let user = self
            .identity()
            .ok_or_else(|| AppError::usage("context requires a stored user anchor"))?;
        let assistant = answer
            .identity()
            .ok_or_else(|| AppError::usage("context requires a stored assistant source"))?;
        if Some(&user) != anchor.identity().as_ref() || assistant.ordinal <= user.ordinal {
            return Err(AppError::database_data(
                "topic context source identity changed",
            ));
        }
        let user_text_chars = self.text.chars().count();
        let assistant_prefix_chars = answer.text.chars().count();
        if user_text_chars > 256 || !(256..=2048).contains(&assistant_prefix_chars) {
            return Err(AppError::usage("invalid topic context text bounds"));
        }
        Ok(Self {
            text: format!("{}\n\n{}", self.text, answer.text),
            kind: TextKind::ContextualRequest,
            origin: Origin::Contextual {
                user,
                assistant,
                user_text_chars,
                user_derived_hash: *blake3::hash(self.text.as_bytes()).as_bytes(),
                assistant_text_kind: answer.kind,
                assistant_prefix_chars,
                assistant_prefix_hash: *blake3::hash(answer.text.as_bytes()).as_bytes(),
                assistant_prefix_complete: answer_complete,
            },
        })
    }

    pub(in crate::topics::evaluation) fn reference_cost(&self) -> usize {
        if matches!(self.origin, Origin::Contextual { .. }) {
            2
        } else {
            1
        }
    }
    pub(in crate::topics::evaluation) fn stored(
        row: &Row<'_>,
        text: String,
    ) -> rusqlite::Result<Self> {
        let canonical: String = row.get("canonical_content")?;
        Ok(Self {
            text,
            origin: Origin::Stored {
                message_id: row.get("message_id")?,
                ordinal: row.get("ordinal")?,
                canonical_hash: *blake3::hash(canonical.as_bytes()).as_bytes(),
            },
            kind: if row.get::<_, bool>("stored_projection")? {
                TextKind::SearchProjection
            } else {
                TextKind::Canonical
            },
        })
    }

    pub(in crate::topics::evaluation) fn projected(&self, text: String) -> Self {
        Self {
            text,
            origin: self.origin.clone(),
            kind: TextKind::TopicProjection,
        }
    }

    pub(super) fn valid(&self) -> bool {
        match &self.origin {
            Origin::Stored {
                message_id,
                ordinal,
                ..
            } => !message_id.is_empty() && message_id.len() <= 256 && *ordinal >= 0,
            Origin::Contextual {
                user,
                assistant,
                user_text_chars,
                assistant_prefix_chars,
                ..
            } => {
                user.valid()
                    && assistant.valid()
                    && assistant.ordinal > user.ordinal
                    && *user_text_chars <= 256
                    && (256..=2048).contains(assistant_prefix_chars)
            }
            Origin::Synthetic => true,
        }
    }
}

impl From<String> for Source {
    fn from(text: String) -> Self {
        Self {
            text,
            origin: Origin::Synthetic,
            kind: TextKind::Synthetic,
        }
    }
}
impl From<&str> for Source {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

#[derive(Serialize)]
pub(super) struct Reference {
    pub(super) origin: Origin,
    pub(super) text_kind: TextKind,
    pub(super) derived_hash: [u8; 32],
    pub(super) text: String,
    text_bytes: usize,
    pub(super) text_complete: bool,
}

impl Reference {
    pub(super) fn capture(source: &Source, remaining: &mut usize) -> Self {
        let mut text = String::new();
        for character in source.text.chars().take(512) {
            if character.len_utf8() > *remaining {
                break;
            }
            text.push(character);
            *remaining -= character.len_utf8();
        }
        Self {
            origin: source.origin.clone(),
            text_kind: source.kind,
            derived_hash: *blake3::hash(source.text.as_bytes()).as_bytes(),
            text_complete: text.len() == source.text.len(),
            text_bytes: source.text.len(),
            text,
        }
    }

    pub(super) fn stored(&self) -> bool {
        matches!(
            self.origin,
            Origin::Stored { .. } | Origin::Contextual { .. }
        )
    }
}
