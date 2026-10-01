//! Bounded reply context for short deictic requests, with fresh grounded embeddings.
mod selection;
#[cfg(test)]
mod tests;
use super::{Thread, labels, names::Labeler, normalize, provenance::Source};
use crate::AppError;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Skip {
    Unstored,
    OversizedAnchor,
    NoAnswer,
    TruncatedWindow,
    AmbiguousBranch,
}

enum Choice {
    Answer {
        anchor: Box<Source>,
        answer: Box<Source>,
        complete: bool,
    },
    Skip(Skip),
}

#[derive(Serialize)]
pub(super) struct Summary {
    eligible_requests: usize,
    enriched_requests: usize,
    unique_embeddings: usize,
    skipped: BTreeMap<Skip, usize>,
    elapsed_ms: f64,
}

fn eligible(text: &str) -> bool {
    if text.chars().count() > 256 || text.contains(['`', '<', '>', '/', '\\']) {
        return false;
    }
    let words: Vec<_> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    words
        .iter()
        .any(|w| ["this", "that", "it", "these", "those"].contains(&w.as_str()))
        && words.iter().all(|w| {
            [
                "what",
                "s",
                "t",
                "do",
                "does",
                "did",
                "is",
                "are",
                "was",
                "were",
                "the",
                "a",
                "an",
                "your",
                "our",
                "you",
                "we",
                "i",
                "me",
                "my",
                "think",
                "opinion",
                "on",
                "of",
                "about",
                "current",
                "state",
                "this",
                "that",
                "it",
                "these",
                "those",
                "pipeline",
                "system",
                "approach",
                "design",
                "idea",
                "work",
                "code",
                "changes",
                "improvements",
                "tests",
                "test",
                "can",
                "could",
                "would",
                "should",
                "tell",
                "explain",
                "mean",
                "means",
                "how",
                "why",
                "help",
                "with",
                "and",
                "to",
                "for",
                "in",
                "hey",
                "so",
                "btw",
                "please",
            ]
            .contains(&w.as_str())
        })
}

struct Pending {
    thread: String,
    index: usize,
    embedding: usize,
    source: Source,
}

pub(super) fn enrich(
    path: &Path,
    threads: &mut BTreeMap<String, Thread>,
    mut labeler: Option<&mut Labeler>,
) -> Result<Summary, AppError> {
    let start = Instant::now();
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(AppError::database)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    let mut summary = Summary {
        eligible_requests: 0,
        enriched_requests: 0,
        unique_embeddings: 0,
        skipped: BTreeMap::new(),
        elapsed_ms: 0.0,
    };
    let mut pending = Vec::new();
    let mut texts = Vec::new();
    let mut unique = BTreeMap::new();
    for (id, thread) in threads.iter() {
        if thread.user_sources.len() != thread.user_messages.len() {
            return Err(AppError::usage("context source alignment mismatch"));
        }
        for (index, source) in thread.user_sources.iter().enumerate() {
            if !eligible(&source.text) {
                continue;
            }
            summary.eligible_requests += 1;
            let (anchor, answer, complete) = match selection::read(&transaction, id, source)? {
                Choice::Answer {
                    anchor,
                    answer,
                    complete,
                } => (anchor, answer, complete),
                Choice::Skip(reason) => {
                    *summary.skipped.entry(reason).or_default() += 1;
                    continue;
                }
            };
            let source = source.with_context(&anchor, &answer, complete)?;
            let embedding = *unique.entry(source.text.clone()).or_insert_with(|| {
                let index = texts.len();
                texts.push(source.text.clone());
                index
            });
            if texts.len() > 1024 {
                return Err(AppError::usage("topic context embedding cap exceeded"));
            }
            pending.push(Pending {
                thread: id.clone(),
                index,
                embedding,
                source,
            });
        }
    }
    drop(transaction);
    let mut vectors = Vec::new();
    if !texts.is_empty() {
        let labeler = labeler
            .as_mut()
            .ok_or_else(|| AppError::model("context requires explicit compatible local models"))?;
        for batch in texts.chunks(8) {
            let references: Vec<_> = batch.iter().map(String::as_str).collect();
            let embedded = labeler.project_vectors(&references)?;
            if embedded.len() != batch.len() {
                return Err(AppError::model("unexpected context embedding count"));
            }
            for vector in embedded {
                vectors.push(normalize(&vector, 384)?);
            }
        }
    }
    summary.unique_embeddings = texts.len();
    summary.enriched_requests = pending.len();
    let phrases = std::env::var_os("MASH_TEST_TOPIC_PHRASES").is_some();
    for plan in pending {
        let thread = threads
            .get_mut(&plan.thread)
            .expect("prepared context thread exists");
        thread.user_messages[plan.index].vector = vectors[plan.embedding].clone();
        thread.user_messages[plan.index].words = if phrases {
            labels::candidates(&plan.source.text)
        } else {
            labels::terms(&plan.source.text)
        };
        thread.user_sources[plan.index] = plan.source;
    }
    summary.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(summary)
}
