//! Offline label-scoring ablation; discovery centroids and assignments stay fixed.
use super::{
    BTreeMap, Cluster, Mode, Thread, TopicId, labels, normalize, partition, thread_samples,
};
use crate::AppError;
use serde::Serialize;
use std::cmp::Ordering;

pub(super) const MAXIMUM_MATCHES: usize = 262_144;
const PER_TOPIC: usize = 4;
const MAXIMUM_SELECTED: usize = 8192;

pub(super) fn enabled() -> bool {
    std::env::var_os("MASH_TEST_TOPIC_REPRESENTATIVE_LABELS").is_some()
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Policy {
    Centroid,
    MatchingSegments,
}

pub(super) fn policy() -> Policy {
    if enabled() {
        Policy::MatchingSegments
    } else {
        Policy::Centroid
    }
}

#[derive(Clone, Serialize)]
pub(super) struct Sample {
    source_id: partition::SourceId,
    thread_id: String,
    segment_index: usize,
    start: usize,
    end: usize,
    lexical_cosine: f64,
    vector: Vec<f64>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SourceScope {
    AllSources,
    TrainingSources,
}

#[derive(Clone, Serialize)]
pub(super) struct Selection {
    source_scope: SourceScope,
    pub(super) center: Vec<f64>,
    samples: Vec<Sample>,
}

fn order(a: &Sample, b: &Sample) -> Ordering {
    b.lexical_cosine
        .total_cmp(&a.lexical_cosine)
        .then(a.source_id.cmp(&b.source_id))
        .then(a.segment_index.cmp(&b.segment_index))
        .then(a.thread_id.cmp(&b.thread_id))
}

fn retain(samples: &mut Vec<Sample>, sample: Sample) {
    if let Some(index) = samples.iter().position(|s| s.source_id == sample.source_id) {
        if order(&sample, &samples[index]).is_lt() {
            samples[index] = sample;
        }
    } else {
        samples.push(sample);
    }
    samples.sort_by(order);
    samples.truncate(PER_TOPIC);
}

fn lexical(words: &labels::Terms, candidates: &[labels::Term]) -> Result<f64, AppError> {
    if words.len() > 8192
        || candidates.len() > 64
        || candidates
            .iter()
            .any(|t| !t.score.is_finite() || t.score < 0.0)
    {
        return Err(AppError::usage("invalid representative topic candidates"));
    }
    if words.is_empty() || candidates.is_empty() {
        return Ok(0.0);
    }
    // Scale first: scores need not be numerically safe to square as supplied.
    let maximum = candidates.iter().map(|t| t.score).fold(0.0, f64::max);
    if maximum == 0.0 {
        return Ok(0.0);
    }
    let norm = candidates
        .iter()
        .map(|t| (t.score / maximum).powi(2))
        .sum::<f64>()
        .sqrt();
    let dot = candidates
        .iter()
        .filter(|t| words.contains_key(&t.text))
        .map(|t| t.score / maximum)
        .sum::<f64>();
    #[allow(
        clippy::cast_precision_loss,
        reason = "bounded offline source vocabulary"
    )]
    let length = (words.len() as f64).sqrt();
    Ok((dot / (norm * length)).clamp(0.0, 1.0))
}

fn ranges(
    thread: &Thread,
    segments: &[super::Segment],
) -> Result<Vec<std::ops::Range<usize>>, AppError> {
    let mut offset: usize = 0;
    let mut ranges = Vec::with_capacity(segments.len());
    for segment in segments {
        if !(1..=128).contains(&segment.messages) {
            return Err(AppError::usage("invalid representative segment size"));
        }
        let start = offset;
        offset = offset
            .checked_add(segment.messages)
            .ok_or_else(|| AppError::usage("invalid representative source range"))?;
        ranges.push(start..offset);
    }
    if offset != thread.user_sources.len() {
        return Err(AppError::usage(
            "representative segments do not cover source sequence",
        ));
    }
    Ok(ranges)
}

fn finish(
    selected: BTreeMap<TopicId, Vec<Sample>>,
    holdout: bool,
) -> Result<BTreeMap<TopicId, Selection>, AppError> {
    selected
        .into_iter()
        .map(|(topic, samples)| {
            let mut sum = vec![0.0; 384];
            for sample in &samples {
                for (total, value) in sum.iter_mut().zip(&sample.vector) {
                    *total += value;
                }
            }
            Ok((
                topic,
                Selection {
                    source_scope: if holdout {
                        SourceScope::TrainingSources
                    } else {
                        SourceScope::AllSources
                    },
                    center: normalize(&sum, 384)?,
                    samples,
                },
            ))
        })
        .collect()
}

fn select(
    threads: &BTreeMap<String, Thread>,
    matching: &BTreeMap<String, BTreeMap<TopicId, Vec<usize>>>,
    mode: Mode,
    holdout: bool,
    clusters: &[Cluster],
) -> Result<BTreeMap<TopicId, Selection>, AppError> {
    if !mode.segmented() || clusters.len() > 8192 {
        return Err(AppError::usage(
            "representative labels require bounded user segments",
        ));
    }
    let candidates: BTreeMap<_, _> = clusters
        .iter()
        .map(|c| (c.id, &c.name.candidates))
        .collect();
    let mut selected: BTreeMap<TopicId, Vec<Sample>> = BTreeMap::new();
    let mut count = 0;
    let mut matches = 0;
    for (id, topics) in matching {
        let thread = threads
            .get(id)
            .ok_or_else(|| AppError::usage("unknown representative thread"))?;
        if holdout && partition::held_out(thread) {
            continue;
        }
        let source_id = thread
            .source_id
            .ok_or_else(|| AppError::usage("representative source identity is missing"))?;
        let segments = thread_samples(thread, mode);
        let ranges = ranges(thread, &segments)?;
        for (topic, indices) in topics {
            let terms = candidates
                .get(topic)
                .ok_or_else(|| AppError::usage("unknown representative topic"))?;
            for &index in indices {
                matches += 1;
                if matches > MAXIMUM_MATCHES {
                    return Err(AppError::usage(
                        "topic label representative match cap exceeded",
                    ));
                }
                let segment = segments
                    .get(index)
                    .ok_or_else(|| AppError::usage("invalid representative segment index"))?;
                let score = lexical(&segment.words, terms)?;
                if score == 0.0 {
                    continue;
                }
                let range = &ranges[index];
                let samples = selected.entry(*topic).or_default();
                let previous = samples.len();
                retain(
                    samples,
                    Sample {
                        source_id,
                        thread_id: id.clone(),
                        segment_index: index,
                        start: range.start,
                        end: range.end,
                        lexical_cosine: score,
                        vector: normalize(&segment.vector, 384)?,
                    },
                );
                count += samples.len() - previous;
                if count > MAXIMUM_SELECTED {
                    return Err(AppError::usage(
                        "topic label representative vector cap exceeded",
                    ));
                }
            }
        }
    }
    finish(selected, holdout)
}

pub(super) fn apply(
    threads: &BTreeMap<String, Thread>,
    matching: &BTreeMap<String, BTreeMap<TopicId, Vec<usize>>>,
    mode: Mode,
    holdout: bool,
    clusters: &mut [Cluster],
) -> Result<(), AppError> {
    // Complete validation before changing any label representation.
    let mut selections = select(threads, matching, mode, holdout, clusters)?;
    for cluster in clusters {
        cluster.name.label_representation = selections.remove(&cluster.id);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
