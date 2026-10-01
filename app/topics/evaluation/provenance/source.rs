use rusqlite::Row;
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(in crate::topics::evaluation) enum Origin {
    Stored {
        message_id: String,
        ordinal: i64,
        canonical_hash: [u8; 32],
    },
    Synthetic,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::topics::evaluation) enum TextKind {
    Canonical,
    SearchProjection,
    TopicProjection,
    Synthetic,
}

#[derive(Clone)]
pub(in crate::topics::evaluation) struct Source {
    pub(in crate::topics::evaluation) text: String,
    pub(super) origin: Origin,
    pub(super) kind: TextKind,
}

impl Source {
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
        matches!(self.origin, Origin::Stored { .. })
    }
}
