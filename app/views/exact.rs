//! Literal retrieval contract, shared independently of embedding recipes.
use crate::AppError;
use aho_corasick::AhoCorasick;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    clap::ValueEnum,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Intent {
    #[default]
    RelatedDiscussion,
    ExactMention,
}

#[derive(Clone)]
pub(crate) struct Literal {
    text: String,
    matcher: AhoCorasick,
    mode: super::matching::Mode,
}

impl Literal {
    pub(crate) fn new(query: &str) -> Result<Self, AppError> {
        let text = query.trim();
        let text = text
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(text);
        if query.len() > 16_384 || !text.chars().any(char::is_alphanumeric) {
            return Err(AppError::usage(
                "exact query must contain letters or numbers and be at most 16384 bytes",
            ));
        }
        let matcher = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build([text])
            .map_err(|_| AppError::internal("build literal matcher"))?;
        Ok(Self {
            text: text.to_owned(),
            matcher,
            mode: super::matching::Mode::Strict,
        })
    }

    pub(crate) fn with_mode(query: &str, mode: super::matching::Mode) -> Result<Self, AppError> {
        let mut literal = Self::new(query)?;
        if !(query.trim().starts_with('"') && query.trim().ends_with('"')) {
            literal.mode = mode;
        }
        Ok(literal)
    }

    pub(crate) fn mode(&self) -> super::matching::Mode {
        self.mode
    }
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn evidence(&self, content: &str) -> Option<super::matching::Evidence> {
        self.locate(content)
            .map(|(range, kind)| super::matching::Evidence {
                kind,
                matched: content[range].to_owned(),
            })
    }

    pub(crate) fn fts_query(&self) -> String {
        format!("\"{}\"", self.text.replace('"', "\"\""))
    }

    pub(crate) fn matches(&self, content: &str) -> bool {
        self.find(content).is_some()
    }

    /// The first boundary-checked occurrence, in UTF-8 byte offsets.
    pub(crate) fn find(&self, content: &str) -> Option<std::ops::Range<usize>> {
        self.locate(content).map(|(range, _)| range)
    }

    fn locate(&self, content: &str) -> Option<(std::ops::Range<usize>, super::matching::Kind)> {
        let exact = self
            .matcher
            .find_overlapping_iter(content)
            .find(|m| {
                !content[..m.start()]
                    .chars()
                    .next_back()
                    .is_some_and(identifier)
                    && !content[m.end()..].chars().next().is_some_and(identifier)
            })
            .map(|m| (m.start()..m.end(), super::matching::Kind::Exact));
        if exact.is_some() || self.mode == super::matching::Mode::Strict {
            return exact;
        }
        let query = words(&self.text);
        if query.is_empty() || query.len() > 32 {
            return None;
        }
        let words = words(content);
        for window in words.windows(query.len()) {
            let mut prefix = false;
            let mut typo = false;
            let mut accepted = true;
            for (index, (q, t)) in query.iter().zip(window).enumerate() {
                if index > 0
                    && self.text[query[index - 1].end..q.start]
                        != content[window[index - 1].end..t.start]
                {
                    accepted = false;
                    break;
                }
                let mode = if index + 1 == query.len() {
                    self.mode
                } else if self.mode.typo() {
                    super::matching::Mode::Typo
                } else {
                    super::matching::Mode::Strict
                };
                match super::matching::relation(&self.text[q.clone()], &content[t.clone()], mode) {
                    Some(super::matching::Kind::Exact) => {}
                    Some(super::matching::Kind::Prefix) => prefix = true,
                    Some(super::matching::Kind::Typo) => typo = true,
                    _ => {
                        accepted = false;
                        break;
                    }
                }
            }
            if accepted {
                let kind = match (prefix, typo) {
                    (true, true) => super::matching::Kind::PrefixAndTypo,
                    (true, false) => super::matching::Kind::Prefix,
                    (false, true) => super::matching::Kind::Typo,
                    _ => super::matching::Kind::Exact,
                };
                return Some((window[0].start..window[window.len() - 1].end, kind));
            }
        }
        None
    }
}

fn words(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut output = Vec::new();
    let mut start = None;
    for (index, c) in text.char_indices() {
        if identifier(c) {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            output.push(start..index);
        }
    }
    if let Some(start) = start {
        output.push(start..text.len());
    }
    output
}

fn identifier(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '+')
}

pub(crate) fn order(hits: &mut [crate::storage::SearchHit]) {
    hits.sort_by(|a, b| {
        quality(a).cmp(&quality(b)).then_with(|| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.id.cmp(&b.id))
        })
    });
}

fn quality(hit: &crate::storage::SearchHit) -> u8 {
    match hit.lexical_match.as_ref().map(|e| e.kind) {
        None | Some(super::matching::Kind::Exact) => 0,
        Some(super::matching::Kind::Prefix) => 1,
        Some(super::matching::Kind::Typo) => 2,
        Some(super::matching::Kind::PrefixAndTypo) => 3,
    }
}

pub(crate) fn rank(
    mut hits: Vec<crate::storage::SearchHit>,
    unit: crate::search_results::SearchUnit,
) -> Vec<crate::storage::SearchHit> {
    order(&mut hits);
    crate::semantic::fuse(hits, vec![], unit, super::config::Fusion::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_preserve_identifiers_phrases_and_overlapping_matches() {
        let literal = Literal::new(" \"voyage-code-4\" ").unwrap();
        for text in ["VOYAGE-code-4", "use (voyage-code-4)."] {
            assert!(literal.matches(text), "{text}");
        }
        for text in [
            "voyage code 4",
            "voyage-code-40",
            "voyage-code-4-v2",
            "voyage-code-4_x",
            "évoyage-code-4",
        ] {
            assert!(!literal.matches(text), "{text}");
        }
        assert!(Literal::new("foo foo").unwrap().matches("xfoo foo foo"));
        assert!(!Literal::new("foo foo").unwrap().matches("foo  foo"));
        let cpp = Literal::new("C++").unwrap();
        assert!(cpp.matches("(c++)"));
        assert!(!cpp.matches("C++20 C+++"));
        assert!(Literal::new("if").unwrap().matches("if (x)"));
        assert!(Literal::new("\"\"").is_err());
        assert!(Literal::new(&"x".repeat(16_385)).is_err());
        assert_eq!(Literal::new("a\"b").unwrap().fts_query(), "\"a\"\"b\"");
    }
}
