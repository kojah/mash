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
        })
    }

    pub(crate) fn fts_query(&self) -> String {
        format!("\"{}\"", self.text.replace('"', "\"\""))
    }

    pub(crate) fn matches(&self, content: &str) -> bool {
        self.matcher.find_overlapping_iter(content).any(|m| {
            !content[..m.start()]
                .chars()
                .next_back()
                .is_some_and(identifier)
                && !content[m.end()..].chars().next().is_some_and(identifier)
        })
    }
}

fn identifier(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '+')
}

pub(crate) fn order(hits: &mut [crate::storage::SearchHit]) {
    hits.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.node_id.cmp(&b.node_id))
            .then_with(|| a.id.cmp(&b.id))
    });
}

pub(crate) fn rank(
    mut hits: Vec<crate::storage::SearchHit>,
    unit: crate::search_results::SearchUnit,
) -> Vec<crate::storage::SearchHit> {
    order(&mut hits);
    crate::semantic::fuse(hits, vec![], unit)
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
