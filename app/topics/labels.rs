//! Class-normalized TF-IDF labels; memberships may contribute to several topics.
use super::TopicId;
use serde::Serialize;
use std::collections::BTreeMap;

pub(super) type Terms = BTreeMap<String, u64>;
#[derive(Serialize)]
pub(super) struct Term {
    pub(super) text: String,
    pub(super) score: f64,
}
pub(super) fn terms(text: &str) -> Terms {
    let mut counts = Terms::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        let word = word.to_lowercase();
        if word.len() >= 3
            && word.len() <= 64
            && !word.bytes().all(|x| x.is_ascii_digit())
            && !STOP.contains(&word.as_str())
        {
            *counts.entry(word).or_default() += 1;
        }
    }
    counts
}
const STOP: &[&str] = &[
    "the", "and", "for", "that", "this", "with", "from", "you", "your", "are", "was", "were",
    "have", "has", "had", "not", "but", "can", "could", "would", "should", "will", "just", "what",
    "when", "where", "how", "into", "they", "their", "them", "then", "than", "also", "about",
    "all", "use", "using", "let", "like", "please", "want", "need", "its", "our", "out", "which",
    "there", "these", "those", "get", "some", "only", "more", "one", "new", "okay", "yes", "well",
    "been", "does", "don", "did", "make", "now", "because", "any", "each", "other",
];

#[allow(
    clippy::cast_precision_loss,
    reason = "bounded prototype corpus term counters become floating-point TF-IDF weights"
)]
pub(super) fn labels(
    classes: &BTreeMap<TopicId, Terms>,
    limit: usize,
) -> BTreeMap<TopicId, Vec<Term>> {
    let mut global = Terms::new();
    let mut totals = BTreeMap::new();
    for (id, words) in classes {
        totals.insert(*id, words.values().sum::<u64>());
        for (word, count) in words {
            *global.entry(word.clone()).or_default() += count;
        }
    }
    let average = totals.values().sum::<u64>() as f64 / classes.len().max(1) as f64;
    classes
        .iter()
        .map(|(id, words)| {
            let total = totals[id].max(1) as f64;
            let mut terms = words
                .iter()
                .map(|(word, count)| Term {
                    text: word.clone(),
                    score: *count as f64 / total * (1.0 + average / global[word] as f64).ln(),
                })
                .collect::<Vec<_>>();
            terms.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.text.cmp(&b.text)));
            terms.truncate(limit);
            (*id, terms)
        })
        .collect()
}
