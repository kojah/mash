//! Bounded, content-free diagnostics for incomplete source snapshots.
use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_SOURCES: usize = 32;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Diagnostics {
    /// First rejected record per source, capped independently of corpus size.
    pub samples: Vec<RecordIssue>,
    pub omitted_sources: u64,
}

impl Diagnostics {
    pub(super) fn record(&mut self, issue: RecordIssue) {
        if self.samples.len() < MAX_SOURCES {
            self.samples.push(issue);
        } else {
            self.omitted_sources += 1;
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordIssue {
    pub provider: Provider,
    pub source: PathBuf,
    /// One-based physical JSONL line, including blank and streamed records.
    pub line: usize,
    /// Serde's byte column within this line (zero for errors before any byte).
    pub column: usize,
    pub category: Category,
}

impl RecordIssue {
    pub(super) fn json(
        provider: Provider,
        source: &Path,
        line: usize,
        error: &serde_json::Error,
    ) -> Self {
        Self {
            provider,
            source: source.to_owned(),
            line,
            column: error.column(),
            category: match error.classify() {
                serde_json::error::Category::Io => Category::Io,
                serde_json::error::Category::Syntax => Category::Syntax,
                serde_json::error::Category::Data => Category::Data,
                serde_json::error::Category::Eof => Category::UnexpectedEof,
            },
        }
    }
}

impl std::fmt::Display for RecordIssue {
    #[allow(clippy::unnecessary_debug_formatting)] // Escape untrusted path control characters.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Debug formatting quotes paths and escapes embedded control characters.
        write!(
            f,
            "{} {:?}:{}:{} ({})",
            self.provider.as_str(),
            self.source,
            self.line,
            self.column,
            self.category
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Category {
    Io,
    Syntax,
    Data,
    UnexpectedEof,
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Io => "io",
            Self::Syntax => "syntax",
            Self::Data => "data",
            Self::UnexpectedEof => "unexpected-eof",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_samples_stay_bounded_and_paths_escape_terminal_controls() {
        let mut diagnostics = Diagnostics::default();
        for line in 1..=100 {
            diagnostics.record(RecordIssue {
                provider: Provider::ClaudeCode,
                source: PathBuf::from("history\n\u{1b}[31m.jsonl"),
                line,
                column: 1,
                category: Category::Syntax,
            });
        }
        assert_eq!(diagnostics.samples.len(), 32);
        assert_eq!(diagnostics.omitted_sources, 68);
        let display = diagnostics.samples[0].to_string();
        assert!(!display.contains('\n'));
        assert!(!display.contains('\u{1b}'));
        let encoded = serde_json::to_string(&diagnostics).unwrap();
        let decoded: Diagnostics = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.samples.len(), 32);
        assert_eq!(decoded.omitted_sources, 68);
    }
}
