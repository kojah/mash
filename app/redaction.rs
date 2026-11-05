mod detection;
pub(crate) use detection::{private, projection, text};
use serde::{Deserialize, Serialize};

/// Where a node masks recognizable credentials, configured under `[redaction]`.
/// Each destination defaults on; credentials in canonical silver are never
/// masked (`<private>` spans never reach it: see [`private_text`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    /// CLI and MCP output: search, view, chat listing and attachments.
    pub(crate) output: bool,
    /// History uploaded to the shared server. Changing it republishes everything.
    pub(crate) publication: bool,
    /// Text sent to remote embedding APIs.
    pub(crate) remote_embeddings: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output: true,
            publication: true,
            remote_embeddings: true,
        }
    }
}

/// Masking for one destination; a disabled policy passes text through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Policy {
    enabled: bool,
}

/// Masking is the default wherever a destination is not configured.
impl Default for Policy {
    fn default() -> Self {
        Self::ON
    }
}

impl Policy {
    pub(crate) const ON: Self = Self { enabled: true };

    pub(crate) const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub(crate) const fn enabled(self) -> bool {
        self.enabled
    }

    pub(crate) fn text(self, value: &str) -> String {
        if self.enabled {
            text(value)
        } else {
            value.to_owned()
        }
    }

    pub(crate) fn projection(self, value: &str) -> String {
        if self.enabled {
            projection(value)
        } else {
            value.to_owned()
        }
    }

    pub(crate) fn metadata(self, value: &mut crate::search_results::ChatMetadata) {
        if self.enabled {
            metadata(value);
        }
    }

    pub(crate) fn ancestors(self, value: &mut [crate::source::Ancestor]) {
        if self.enabled {
            ancestors(value);
        }
    }

    pub(crate) fn attachments(self, value: &mut [crate::attachments::Metadata]) {
        if self.enabled {
            attachments(value);
        }
    }

    pub(crate) fn tools(self, value: &mut crate::tools::Activity) {
        if self.enabled {
            value.redact();
        }
    }

    pub(crate) fn batch(self, value: &mut crate::shared::protocol::Batch) {
        if self.enabled {
            batch(value);
        }
    }
}

impl Settings {
    pub(crate) const fn output(self) -> Policy {
        Policy::new(self.output)
    }

    pub(crate) const fn publication(self) -> Policy {
        Policy::new(self.publication)
    }

    pub(crate) const fn remote_embeddings(self) -> Policy {
        Policy::new(self.remote_embeddings)
    }
}

/// Masks `<private>` spans before silver stores a value, so no index, view,
/// export or publication derived from silver holds them.
pub(crate) fn private_text(value: &mut String) {
    if let std::borrow::Cow::Owned(masked) = private(value) {
        *value = masked;
    }
}

/// Copies a message only when it holds a `<private>` tag.
pub(crate) fn private_message(
    message: &mut std::borrow::Cow<'_, crate::silver::NormalizedMessage>,
) {
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

/// Ancestor titles are copies of container metadata and need the same masking.
pub(crate) fn ancestors(ancestors: &mut [crate::source::Ancestor]) {
    for title in ancestors
        .iter_mut()
        .filter_map(|ancestor| ancestor.title.as_mut())
    {
        *title = text(title);
    }
}

pub(crate) fn metadata(metadata: &mut crate::search_results::ChatMetadata) {
    let mut changed = false;
    for value in [
        &mut metadata.title,
        &mut metadata.project,
        &mut metadata.repository,
    ]
    .into_iter()
    .flatten()
    {
        let safe = text(value);
        changed |= safe != *value;
        *value = safe;
    }
    if changed {
        metadata.enrichment = None;
    }
}

pub(crate) fn conversation(record: &mut crate::shared::protocol::ConversationRecord) {
    let mut changed = false;
    for value in [
        &mut record.title,
        &mut record.project,
        &mut record.repository,
    ]
    .into_iter()
    .flatten()
    {
        let safe = text(value);
        changed |= safe != *value;
        *value = safe;
    }
    if changed {
        record.enrichment = None;
    }
}

pub(crate) fn batch(batch: &mut crate::shared::protocol::Batch) {
    use crate::shared::protocol::Batch;
    match batch {
        Batch::Conversations { records } => records.iter_mut().for_each(conversation),
        Batch::Messages { records } => {
            for record in records {
                record.content = text(&record.content);
                record.projection = text(&record.projection);
                attachments(&mut record.attachments);
                record.tools.redact();
            }
        }
        Batch::Tombstones { .. } => {}
    }
}

pub(crate) fn attachments(records: &mut [crate::attachments::Metadata]) {
    for record in records {
        record.name = text(&record.name);
        record.media_type = text(&record.media_type);
    }
}

#[cfg(test)]
mod tests;
