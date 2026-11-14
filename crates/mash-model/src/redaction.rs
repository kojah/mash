mod detection;
pub use detection::{private, projection, text};
use serde::{Deserialize, Serialize};

/// Where a node masks recognizable credentials, configured under `[redaction]`.
/// Each destination defaults on; credentials in canonical silver are never
/// masked (`<private>` spans never reach it: see [`private_text`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "RedactionSettings")]
pub struct Settings {
    /// CLI and MCP output: search, view, chat listing and attachments.
    pub output: bool,
    /// History uploaded to the shared server. Changing it republishes everything.
    pub publication: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output: true,
            publication: true,
        }
    }
}

/// Masking for one destination; a disabled policy passes text through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    enabled: bool,
}

/// Masking is the default wherever a destination is not configured.
impl Default for Policy {
    fn default() -> Self {
        Self::ON
    }
}

impl Policy {
    pub const ON: Self = Self { enabled: true };

    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub const fn enabled(self) -> bool {
        self.enabled
    }

    pub fn text(self, value: &str) -> String {
        if self.enabled {
            text(value)
        } else {
            value.to_owned()
        }
    }

    pub fn projection(self, value: &str) -> String {
        if self.enabled {
            projection(value)
        } else {
            value.to_owned()
        }
    }

    pub fn attachments(self, value: &mut [crate::attachments::Metadata]) {
        if self.enabled {
            attachments(value);
        }
    }

    pub fn tools(self, value: &mut crate::tools::Activity) {
        if self.enabled {
            value.redact();
        }
    }
}

impl Settings {
    pub const fn output(self) -> Policy {
        Policy::new(self.output)
    }

    pub const fn publication(self) -> Policy {
        Policy::new(self.publication)
    }
}

/// Masks `<private>` spans before silver stores a value, so no index, view,
/// export or publication derived from silver holds them.
pub fn private_text(value: &mut String) {
    if let std::borrow::Cow::Owned(masked) = private(value) {
        *value = masked;
    }
}

pub fn attachments(records: &mut [crate::attachments::Metadata]) {
    for record in records {
        record.name = text(&record.name);
        record.media_type = text(&record.media_type);
    }
}

#[cfg(test)]
mod tests;
