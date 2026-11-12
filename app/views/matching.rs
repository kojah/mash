//! Bounded lexical query expansion shared by SQLite and Qdrant.
use rmcp::schemars;
use serde::{Deserialize, Serialize};

pub(crate) const VOCABULARY_LIMIT: usize = 100_000;
const ALTERNATIVES: usize = 8;
/// Candidates a scoped plan checks per query word, best-ranked first.
const MAX_CHECKS: usize = 64;

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
pub(crate) enum Mode {
    Strict,
    Prefix,
    Typo,
    #[default]
    Flexible,
}

impl Mode {
    pub(crate) fn prefix(self) -> bool {
        matches!(self, Self::Prefix | Self::Flexible)
    }
    pub(crate) fn typo(self) -> bool {
        matches!(self, Self::Typo | Self::Flexible)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Evidence {
    pub(crate) kind: Kind,
    pub(crate) matched: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    Exact,
    Prefix,
    Typo,
    PrefixAndTypo,
}

pub(crate) fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// Exact numbers remain exact even inside version strings and identifiers.
pub(crate) fn relation(query: &str, term: &str, mode: Mode) -> Option<Kind> {
    if query.eq_ignore_ascii_case(term) {
        return Some(Kind::Exact);
    }
    if !query.is_ascii() || !term.is_ascii() || digits(query) != digits(term) {
        return None;
    }
    if mode.prefix()
        && query.len() >= 3
        && !crate::search_query::is_stop_word(query)
        && term.len() > query.len()
        && term[..query.len()].eq_ignore_ascii_case(query)
    {
        return Some(Kind::Prefix);
    }
    if mode.typo()
        && query.len() >= 5
        && query.len() <= 64
        && term.len() <= 64
        && !crate::search_query::is_stop_word(query)
        && distance(
            query.as_bytes(),
            term.as_bytes(),
            if query.len() >= 9 { 2 } else { 1 },
        )
    {
        return Some(Kind::Typo);
    }
    None
}

/// Optimal-string-alignment distance, including adjacent transpositions.
fn distance(left: &[u8], right: &[u8], budget: usize) -> bool {
    if left.len().abs_diff(right.len()) > budget {
        return false;
    }
    let mut older = vec![0; right.len() + 1];
    let mut previous: Vec<_> = (0..=right.len()).collect();
    for (i, &a) in left.iter().enumerate() {
        let mut current = vec![i + 1; right.len() + 1];
        for (j, &b) in right.iter().enumerate() {
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + usize::from(!a.eq_ignore_ascii_case(&b)));
            if i > 0
                && j > 0
                && a.eq_ignore_ascii_case(&right[j - 1])
                && left[i - 1].eq_ignore_ascii_case(&b)
            {
                current[j + 1] = current[j + 1].min(older[j - 1] + 1);
            }
        }
        if current.iter().copied().min().unwrap_or(budget + 1) > budget {
            return false;
        }
        older = previous;
        previous = current;
    }
    previous[right.len()] <= budget
}

pub(crate) struct Plan {
    pub(crate) groups: Vec<Vec<String>>,
}

impl Plan {
    /// Report an actual expanded indexed term, never an inferred correction.
    pub(crate) fn evidence(&self, content: &str, mode: Mode) -> Option<Evidence> {
        let terms = tokens(content);
        if self.groups.iter().all(|group| terms.contains(&group[0])) {
            return None;
        }
        for term in terms {
            for group in &self.groups {
                if group.iter().skip(1).any(|alternative| alternative == &term)
                    && let Some(kind) = relation(&group[0], &term, mode)
                    && kind != Kind::Exact
                {
                    return Some(Evidence {
                        kind,
                        matched: term,
                    });
                }
            }
        }
        None
    }
    pub(crate) fn new(
        query: &str,
        mode: Mode,
        vocabulary: &[String],
    ) -> Result<Self, crate::AppError> {
        Self::scoped(query, mode, vocabulary, |_| Ok(true))
    }

    /// Like `new`, keeping only alternatives that `occurs` reports in scope.
    /// Candidates are checked in rank order until each word has its quota.
    pub(crate) fn scoped(
        query: &str,
        mode: Mode,
        vocabulary: &[String],
        mut occurs: impl FnMut(&str) -> Result<bool, crate::AppError>,
    ) -> Result<Self, crate::AppError> {
        let quoted = query.trim().starts_with('"') && query.trim().ends_with('"');
        let mode = if quoted { Mode::Strict } else { mode };
        let words = tokens(query);
        if mode != Mode::Strict && words.len() > 32 {
            return Err(crate::AppError::usage(
                "lexical expansion supports at most 32 query terms; shorten the query or use match_mode=strict",
            ));
        }
        let groups = words
            .into_iter()
            .map(|word| {
                let mut alternatives: Vec<_> = vocabulary
                    .iter()
                    .filter_map(|term| match relation(&word, term, mode) {
                        Some(Kind::Prefix) => Some((0, term)),
                        Some(Kind::Typo) => Some((1, term)),
                        _ => None,
                    })
                    .collect();
                alternatives.sort_unstable();
                alternatives.dedup();
                let mut group = vec![word];
                // Each check can scan a word's matches; bound them so a
                // selective scope cannot turn one query into thousands.
                for (_, term) in alternatives.into_iter().take(MAX_CHECKS) {
                    if group.len() > ALTERNATIVES {
                        break;
                    }
                    if occurs(term)? {
                        group.push(term.clone());
                    }
                }
                Ok(group)
            })
            .collect::<Result<_, crate::AppError>>()?;
        Ok(Self { groups })
    }

    /// Every accepted word once, so a repeated word is not scored twice.
    pub(crate) fn query_text(&self) -> String {
        let mut seen = std::collections::BTreeSet::new();
        self.groups
            .iter()
            .flatten()
            .filter(|word| seen.insert(word.as_str()))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Evidence never names text the destination's policy masks.
pub(crate) fn annotate(
    hit: &mut crate::storage::SearchHit,
    literal: &super::exact::Literal,
    redaction: crate::redaction::Policy,
) {
    let safe = redaction.text(&hit.content);
    hit.lexical_match = literal.evidence(&safe);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_bound_expansion_and_keep_exact_evidence_distinct() {
        let words = vec!["idempotency".to_owned(), "idempotent".to_owned()];
        let plan = Plan::new("idemp", Mode::Flexible, &words).unwrap();
        assert_eq!(
            plan.evidence("idempotency", Mode::Flexible).unwrap().kind,
            Kind::Prefix
        );
        assert!(plan.evidence("idemp idempotency", Mode::Flexible).is_none());
        let large = vec!["database"; 33].join(" ");
        assert!(Plan::new(&large, Mode::Flexible, &words).is_err());
        assert!(Plan::new(&large, Mode::Strict, &[]).is_ok());
        let quoted = Plan::new("\"idemp\"", Mode::Flexible, &words).unwrap();
        assert_eq!(quoted.groups, vec![vec!["idemp".to_owned()]]);
    }
    #[test]
    fn typo_prefix_and_numeric_guards() {
        assert_eq!(
            relation("idemp", "idempotency", Mode::Flexible),
            Some(Kind::Prefix)
        );
        assert_eq!(
            relation("idempotnecy", "idempotency", Mode::Flexible),
            Some(Kind::Typo)
        );
        assert_eq!(relation("redis", "reads", Mode::Strict), None);
        assert_eq!(relation("redis4", "redis40", Mode::Flexible), None);
        assert_eq!(relation("139", "1399", Mode::Flexible), None);
        assert_eq!(relation("s3", "s4", Mode::Flexible), None);
        assert_eq!(relation("the", "there", Mode::Flexible), None);
    }
}
