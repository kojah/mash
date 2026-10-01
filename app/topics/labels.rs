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
        if (3..=64).contains(&word.len())
            && word.chars().next().is_some_and(char::is_alphabetic)
            && !(word.len() >= 12 && word.bytes().all(|x| x.is_ascii_hexdigit()))
            && !STOP.contains(&word.as_str())
        {
            *counts.entry(word).or_default() += 1;
        }
    }
    counts
}

#[test]
fn label_candidates_exclude_timestamp_and_digest_noise_but_keep_technical_names() {
    let terms = terms(
        "00z 10t00 12345 90e14213b066b7bc38fe4a02946b16d027fc9d84cb8ab7a3367a32dbe1362a70 abcdefabcdefabcdef pgvector cbioportal gpt4 bm25 x86",
    );
    assert_eq!(
        terms.keys().map(String::as_str).collect::<Vec<_>>(),
        ["bm25", "cbioportal", "gpt4", "pgvector", "x86"]
    );
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
