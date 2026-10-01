//! Reserve core words alongside diverse phrases before semantic scoring.
use super::{Term, Terms};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::topics) enum Policy {
    Tfidf,
    Balanced,
    Supported,
}

pub(in crate::topics) fn policy() -> Policy {
    if std::env::var_os("MASH_TEST_TOPIC_SUPPORTED_LABELS").is_some() {
        Policy::Supported
    } else if std::env::var_os("MASH_TEST_TOPIC_BALANCED_LABELS").is_some() {
        Policy::Balanced
    } else {
        Policy::Tfidf
    }
}

fn contained(a: &str, b: &str) -> bool {
    let a: Vec<_> = a.split_whitespace().collect();
    let b: Vec<_> = b.split_whitespace().collect();
    a.len() <= b.len() && b.windows(a.len()).any(|part| part == a)
}

pub(super) fn select(
    mut terms: Vec<Term>,
    counts: &Terms,
    limit: usize,
    policy: Policy,
) -> Vec<Term> {
    if matches!(policy, Policy::Supported) {
        return supported(terms, limit);
    }
    if matches!(policy, Policy::Tfidf) {
        terms.truncate(limit);
        return terms;
    }
    let mut words: Vec<_> = terms
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.text.contains(' '))
        .collect();
    words.sort_by(|(_, a), (_, b)| {
        counts[&b.text]
            .cmp(&counts[&a.text])
            .then(b.score.total_cmp(&a.score))
            .then(a.text.cmp(&b.text))
    });
    let mut selected: BTreeSet<_> = words
        .into_iter()
        .take(limit / 2)
        .map(|(index, _)| index)
        .collect();
    let mut phrases: Vec<&str> = Vec::new();
    for (index, term) in terms.iter().enumerate() {
        if selected.len() == limit {
            break;
        }
        if term.text.contains(' ')
            && !phrases
                .iter()
                .any(|prior| contained(&term.text, prior) || contained(prior, &term.text))
        {
            phrases.push(&term.text);
            selected.insert(index);
        }
    }
    // Use spare capacity for small vocabularies rather than dropping evidence.
    for index in 0..terms.len() {
        if selected.len() == limit {
            break;
        }
        selected.insert(index);
    }
    terms
        .into_iter()
        .enumerate()
        .filter_map(|(index, t)| selected.contains(&index).then_some(t))
        .collect()
}

/// A repeated subject can survive a long tail of unique phrases. Keep the
/// original c-TF-IDF for semantic ranking and expose support independently.
#[allow(
    clippy::cast_precision_loss,
    reason = "bounded prototype term counters"
)]
fn supported(mut terms: Vec<Term>, limit: usize) -> Vec<Term> {
    terms.retain(|t| t.support > 0 && !super::discourse::only(&t.text));
    let maximum = terms.iter().map(|t| t.score).fold(0.0, f64::max);
    let support = terms.iter().map(|t| t.support).max().unwrap_or(1) as f64;
    let score = |t: &Term| {
        let relative = if maximum > 0.0 {
            t.score / maximum
        } else {
            0.0
        };
        0.3 * relative + 0.7 * (t.support as f64 / support)
    };
    terms.sort_by(|a, b| {
        score(b)
            .total_cmp(&score(a))
            .then(b.score.total_cmp(&a.score))
            .then(a.text.cmp(&b.text))
    });
    terms.truncate(limit);
    terms
}

#[test]
fn repeated_core_terms_survive_rare_phrase_saturation() {
    let mut terms: Vec<_> = (0..80)
        .map(|i| Term {
            text: format!("rare phrase {i}"),
            score: 1.0,
            support: 1,
        })
        .collect();
    terms.extend([
        Term {
            text: "deadlock".into(),
            score: 0.1,
            support: 2,
        },
        Term {
            text: "runtime".into(),
            score: 0.09,
            support: 2,
        },
    ]);
    let mut counts: Terms = terms.iter().map(|t| (t.text.clone(), 1)).collect();
    counts.insert("deadlock".into(), 2);
    counts.insert("runtime".into(), 2);
    let raw = select(terms.clone(), &counts, 64, Policy::Tfidf);
    assert!(!raw.iter().any(|t| t.text == "deadlock"));
    let balanced = select(terms, &counts, 64, Policy::Balanced);
    assert_eq!(balanced.len(), 64);
    assert!(balanced.iter().any(|t| t.text == "deadlock"));
    assert!(balanced.iter().any(|t| t.text == "runtime"));
}

#[test]
fn diverse_phrases_leave_space_for_other_subjects_and_small_vocabularies() {
    let terms: Vec<_> = [
        "database connection pool",
        "database connection",
        "connection pool",
        "frontend layout",
        "postgres",
    ]
    .into_iter()
    .map(|text| Term {
        text: text.into(),
        score: 1.0,
        support: 1,
    })
    .collect();
    let counts = terms.iter().map(|t| (t.text.clone(), 1)).collect();
    let selected = select(terms.clone(), &counts, 3, Policy::Balanced);
    assert_eq!(
        selected.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
        ["database connection pool", "frontend layout", "postgres"]
    );
    assert_eq!(select(terms, &counts, 64, Policy::Balanced).len(), 5);
}

#[test]
fn support_admits_repeated_subjects_without_erasing_rare_topics_or_named_greetings() {
    let mut terms = (0..80)
        .map(|i| Term {
            text: format!("incidental phrase {i}"),
            score: 1.0,
            support: 1,
        })
        .collect::<Vec<_>>();
    terms.extend([
        Term {
            text: "database".into(),
            score: 0.1,
            support: 2,
        },
        Term {
            text: "hey".into(),
            score: 2.0,
            support: 2,
        },
        Term {
            text: "sounds good".into(),
            score: 2.0,
            support: 2,
        },
    ]);
    let counts = terms.iter().map(|t| (t.text.clone(), t.support)).collect();
    let selected = select(terms, &counts, 64, Policy::Supported);
    assert_eq!(selected.len(), 64);
    assert_eq!(selected[0].text, "database");
    assert!(
        !selected
            .iter()
            .any(|t| matches!(t.text.as_str(), "hey" | "sounds good"))
    );
    let rare = ["sqlite vec", "hello world", "go language"]
        .into_iter()
        .map(|text| Term {
            text: text.into(),
            score: 1.0,
            support: 1,
        })
        .collect::<Vec<_>>();
    let counts = rare.iter().map(|t| (t.text.clone(), t.support)).collect();
    assert_eq!(
        select(rare.clone(), &counts, 64, Policy::Supported).len(),
        rare.len()
    );
    assert_eq!(select(rare, &counts, 0, Policy::Supported).len(), 0);
}
