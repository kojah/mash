//! Explicit private evidence capture; never stdout/traces or production payloads.
mod source;
use super::{BTreeMap, BTreeSet, Mode, Thread, TopicId, partition::SourceId};
use crate::{AppError, provider::Provider};
use serde::{Deserialize, Serialize};
pub(super) use source::Source;
use std::io::Read;

const MAXIMUM_THREADS: usize = 64;
const MAXIMUM_TOPICS: usize = 64;
const MAXIMUM_SEGMENTS: usize = 4;
const MAXIMUM_REFERENCES: usize = 16_384;
const MAXIMUM_TEXT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    threads: Vec<String>,
}

pub(super) struct Selection(BTreeSet<String>);
impl Selection {
    pub(super) fn load() -> Result<Option<Self>, AppError> {
        let Some(path) = std::env::var_os("MASH_TEST_TOPIC_EVIDENCE_THREADS") else {
            return Ok(None);
        };
        let file = std::fs::File::open(path)
            .map_err(|_| AppError::usage("cannot read topic evidence selection"))?;
        let mut bytes = Vec::new();
        file.take(16_385)
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::usage("cannot read topic evidence selection"))?;
        if bytes.len() > 16_384 {
            return Err(AppError::usage(
                "topic evidence selection exceeds byte limit",
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| AppError::usage("invalid topic evidence selection"))?;
        Self::parse(text).map(Some)
    }

    pub(super) fn parse(text: &str) -> Result<Self, AppError> {
        let input: Input = toml::from_str(text)
            .map_err(|_| AppError::usage("invalid topic evidence selection"))?;
        let threads: BTreeSet<_> = input.threads.iter().cloned().collect();
        if !(1..=MAXIMUM_THREADS).contains(&threads.len())
            || threads.len() != input.threads.len()
            || threads.iter().any(|id| id.is_empty() || id.len() > 256)
        {
            return Err(AppError::usage("invalid topic evidence thread selection"));
        }
        Ok(Self(threads))
    }

    pub(super) fn validate(&self, threads: &BTreeMap<String, Thread>) -> Result<(), AppError> {
        if self.0.iter().any(|id| !threads.contains_key(id)) {
            return Err(AppError::usage("unknown topic evidence thread"));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize)]
pub(super) struct SegmentScore {
    segment_index: usize,
    start: usize,
    end: usize,
    cosine: f64,
    support: f64,
    weight: f64,
}
impl SegmentScore {
    pub(super) fn new(
        segment_index: usize,
        start: usize,
        end: usize,
        cosine: f64,
        support: f64,
        weight: f64,
    ) -> Self {
        Self {
            segment_index,
            start,
            end,
            cosine,
            support,
            weight,
        }
    }
}

#[derive(Default)]
pub(super) struct Scores {
    segments: usize,
    messages: usize,
    top: Vec<SegmentScore>,
}
impl Scores {
    pub(super) fn add(&mut self, score: SegmentScore) {
        self.segments += 1;
        self.messages += score.end - score.start;
        self.top.push(score);
        self.top.sort_by(|a, b| {
            b.support
                .total_cmp(&a.support)
                .then(b.cosine.total_cmp(&a.cosine))
                .then(a.segment_index.cmp(&b.segment_index))
        });
        self.top.truncate(MAXIMUM_SEGMENTS);
    }
}

#[derive(Serialize)]
struct Sample {
    #[serde(flatten)]
    score: SegmentScore,
    complete_derived_text: bool,
    complete_canonical_references: bool,
}
#[derive(Serialize)]
struct Topic {
    id: TopicId,
    support: f64,
    matching_segments: usize,
    supporting_messages: usize,
    omitted_segments: usize,
    samples: Vec<Sample>,
}
#[derive(Serialize)]
struct Evidence {
    source_id: Option<SourceId>,
    provider: Provider,
    source_messages_total: usize,
    omitted_sources: usize,
    sources: Vec<source::Reference>,
    retained_topics_total: usize,
    omitted_topics: usize,
    topics: Vec<Topic>,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    UserEvidence,
    UnsupportedBaseline,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SourceScope {
    RetainedEmbeddedUserMessages,
    ContextualUserRequests,
}

#[derive(Serialize)]
pub(super) struct Report {
    schema_version: u32,
    state: State,
    source_scope: SourceScope,
    selected_threads: usize,
    text_bytes: usize,
    source_references: usize,
    threads: BTreeMap<String, Evidence>,
}

#[cfg(test)]
impl Report {
    pub(super) fn topic_ids(&self, thread: &str) -> Vec<TopicId> {
        self.threads[thread]
            .topics
            .iter()
            .map(|topic| topic.id)
            .collect()
    }
}

pub(super) struct Collector<'a> {
    selection: &'a Selection,
    supported: bool,
    text_remaining: usize,
    refs_remaining: usize,
    report: Report,
}
impl<'a> Collector<'a> {
    pub(super) fn new(selection: &'a Selection, mode: Mode) -> Self {
        let supported = !matches!(mode, Mode::ThreadMean | Mode::Segments);
        Self {
            selection,
            supported,
            text_remaining: MAXIMUM_TEXT_BYTES,
            refs_remaining: MAXIMUM_REFERENCES,
            report: Report {
                schema_version: 3,
                state: if supported {
                    State::UserEvidence
                } else {
                    State::UnsupportedBaseline
                },
                source_scope: SourceScope::RetainedEmbeddedUserMessages,
                selected_threads: selection.0.len(),
                text_bytes: 0,
                source_references: 0,
                threads: BTreeMap::new(),
            },
        }
    }

    pub(super) fn selected(&self, id: &str) -> bool {
        self.supported && self.selection.0.contains(id)
    }

    pub(super) fn validate(thread: &Thread) -> Result<(), AppError> {
        if thread.user_sources.len() != thread.user_messages.len()
            || thread.user_messages.iter().any(|m| m.messages != 1)
            || thread.user_sources.iter().any(|s| !s.valid())
        {
            return Err(AppError::usage(
                "topic evidence source alignment is invalid",
            ));
        }
        Ok(())
    }

    pub(super) fn record(
        &mut self,
        id: &str,
        thread: &Thread,
        mut scores: Vec<(TopicId, f64, Scores)>,
    ) {
        let total = scores.len();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scores.truncate(MAXIMUM_TOPICS);
        let previous_text_remaining = self.text_remaining;
        let mut sources = Vec::new();
        for source in &thread.user_sources {
            let cost = source.reference_cost();
            if cost > self.refs_remaining {
                break;
            }
            self.refs_remaining -= cost;
            self.report.source_references += cost;
            if cost == 2 {
                self.report.source_scope = SourceScope::ContextualUserRequests;
            }
            sources.push(source::Reference::capture(source, &mut self.text_remaining));
        }
        self.report.text_bytes += previous_text_remaining - self.text_remaining;
        let topics = scores
            .into_iter()
            .map(|(id, support, scores)| {
                let samples: Vec<_> = scores
                    .top
                    .into_iter()
                    .map(|score| {
                        let refs = sources.get(score.start..score.end);
                        Sample {
                            complete_derived_text: refs
                                .is_some_and(|s| s.iter().all(|r| r.text_complete)),
                            complete_canonical_references: refs
                                .is_some_and(|s| s.iter().all(source::Reference::stored)),
                            score,
                        }
                    })
                    .collect();
                Topic {
                    id,
                    support,
                    matching_segments: scores.segments,
                    supporting_messages: scores.messages,
                    omitted_segments: scores.segments - samples.len(),
                    samples,
                }
            })
            .collect::<Vec<_>>();
        self.report.threads.insert(
            id.to_owned(),
            Evidence {
                source_id: thread.source_id,
                provider: thread.provider,
                source_messages_total: thread.user_sources.len(),
                omitted_sources: thread.user_sources.len() - sources.len(),
                sources,
                retained_topics_total: total,
                omitted_topics: total - topics.len(),
                topics,
            },
        );
    }

    pub(super) fn finish(self) -> Report {
        self.report
    }
}

#[cfg(test)]
mod tests;
