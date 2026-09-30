//! Result granularity and bounded conversation ranking, shared by all transports.
use crate::provider::Provider;
use crate::storage::SearchHit;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(
    Debug, Clone, Copy, Default, Deserialize, Serialize, clap::ValueEnum, rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SearchUnit {
    #[default]
    Threads,
    Messages,
}

impl SearchUnit {
    pub(crate) const fn candidate_limit(self) -> usize {
        match self {
            Self::Threads => 200,
            Self::Messages => 50,
        }
    }
    pub(crate) const fn rerank_limit(self) -> usize {
        match self {
            Self::Threads => 30,
            Self::Messages => 10,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct ThreadMetadata {
    pub(crate) title: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) repository: Option<String>,
    pub(crate) created_at: Option<i64>,
    pub(crate) updated_at: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum Results {
    Threads(Vec<ThreadHit>),
    Messages(Vec<SearchHit>),
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ThreadHit {
    pub(crate) conversation_id: String,
    pub(crate) provider: Provider,
    pub(crate) node_id: Option<String>,
    #[serde(flatten)]
    pub(crate) metadata: ThreadMetadata,
    pub(crate) score: f32,
    /// Up to three distinct matching excerpts, ordered by relevance.
    pub(crate) matches: Vec<ThreadMatch>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ThreadMatch {
    pub(crate) message_id: String,
    pub(crate) node_id: Option<String>,
    pub(crate) role: crate::role::Role,
    pub(crate) created_at: Option<i64>,
    pub(crate) snippet: String,
    pub(crate) rerank_score: Option<f32>,
    pub(crate) fusion_score: f32,
    /// Other indexed copies of this evidence, usable as view anchors.
    pub(crate) copies: Vec<MessageReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct MessageReference {
    pub(crate) message_id: String,
    pub(crate) node_id: Option<String>,
}

pub(crate) type EvidenceKey = (Provider, String, crate::role::Role, blake3::Hash);

pub(crate) fn evidence_key(hit: &SearchHit) -> EvidenceKey {
    let text = hit.content.split_whitespace().collect::<Vec<_>>().join(" ");
    (
        hit.provider,
        hit.conversation_id.clone(),
        hit.role.clone(),
        blake3::hash(text.as_bytes()),
    )
}

/// Apply current exclusions even to snapshots published by older nodes.
pub(crate) fn eligible(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    hits.into_iter()
        .filter_map(|mut hit| {
            let clean = crate::search_text::project(&hit.role, &hit.content);
            if clean.trim().is_empty() {
                return None;
            }
            if let std::borrow::Cow::Owned(clean) = clean {
                hit.content = clean;
            }
            hit.metadata.title = hit
                .metadata
                .title
                .as_deref()
                .and_then(crate::search_text::title);
            Some(hit)
        })
        .collect()
}

pub(crate) fn merge_copies(existing: &mut SearchHit, other: SearchHit) {
    let original = MessageReference {
        message_id: existing.id.clone(),
        node_id: existing.node_id.clone(),
    };
    for reference in std::iter::once(MessageReference {
        message_id: other.id,
        node_id: other.node_id,
    })
    .chain(other.copies)
    {
        if reference != original && !existing.copies.contains(&reference) {
            existing.copies.push(reference);
        }
    }
}

pub(crate) fn deduplicate(hits: Vec<SearchHit>, unit: SearchUnit) -> Vec<SearchHit> {
    let hits = eligible(hits);
    if matches!(unit, SearchUnit::Messages) {
        return hits;
    }
    let mut positions = HashMap::new();
    let mut unique = Vec::<SearchHit>::new();
    for hit in hits {
        let key = evidence_key(&hit);
        if let Some(&index) = positions.get(&key) {
            merge_copies(&mut unique[index], hit);
        } else {
            positions.insert(key, unique.len());
            unique.push(hit);
        }
    }
    unique
}

/// Round-robin the best candidates from each conversation before bounded reranking.
/// Identical content (including copies on other nodes) earns no additional evidence.
pub(crate) fn diversify(hits: Vec<SearchHit>, unit: SearchUnit) -> Vec<SearchHit> {
    let hits = deduplicate(hits, unit);
    if matches!(unit, SearchUnit::Messages) {
        return hits;
    }
    let mut counts = HashMap::new();
    let mut rounds: [Vec<SearchHit>; 3] = Default::default();
    for hit in hits {
        let key = (hit.provider, hit.conversation_id.clone());
        let count = counts.entry(key).or_insert(0);
        if *count < 3 {
            rounds[*count].push(hit);
            *count += 1;
        }
    }
    rounds.into_iter().flatten().collect()
}

pub(crate) fn collect(
    hits: Vec<SearchHit>,
    unit: SearchUnit,
    limit: usize,
    query: &str,
) -> Results {
    if matches!(unit, SearchUnit::Messages) {
        return Results::Messages(eligible(hits).into_iter().take(limit.min(1000)).collect());
    }
    let mut threads = Vec::<ThreadHit>::new();
    let mut groups = HashMap::new();
    for (rank, hit) in deduplicate(hits, unit).into_iter().enumerate() {
        let key = (hit.provider, hit.conversation_id.clone());
        let index = *groups.entry(key.clone()).or_insert_with(|| {
            threads.push(ThreadHit {
                conversation_id: hit.conversation_id.clone(),
                provider: hit.provider,
                node_id: hit.node_id.clone(),
                metadata: ThreadMetadata {
                    title: hit
                        .metadata
                        .title
                        .clone()
                        .or_else(|| crate::search_text::title(&hit.content)),
                    ..hit.metadata.clone()
                },
                score: 0.0,
                matches: Vec::new(),
            });
            threads.len() - 1
        });
        let thread = &mut threads[index];
        if thread.matches.len() == 3 {
            continue;
        }
        // Reciprocal rank preserves the existing reranker/fusion ordering without
        // interpreting uncalibrated logits as probabilities. Corroboration is capped.
        let rank = f32::from(u16::try_from(rank).unwrap_or(u16::MAX));
        let weight = [1.0, 0.1, 0.05][thread.matches.len()];
        thread.score += weight / (60.0 + rank + 1.0);
        thread.matches.push(ThreadMatch {
            message_id: hit.id,
            node_id: hit.node_id,
            role: hit.role,
            created_at: hit.created_at,
            snippet: excerpt(&hit.content, query),
            rerank_score: hit.rerank_score,
            fusion_score: hit.fusion_score,
            copies: hit.copies,
        });
    }
    threads.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.provider.as_str().cmp(b.provider.as_str()))
            .then_with(|| a.conversation_id.cmp(&b.conversation_id))
    });
    threads.truncate(limit.min(1000));
    Results::Threads(threads)
}

/// Prefer a window around lexical evidence; semantic-only matches use the start.
fn excerpt(content: &str, query: &str) -> String {
    let offset = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 3 && word.is_ascii())
        .filter_map(|word| {
            content
                .as_bytes()
                .windows(word.len())
                .position(|window| window.eq_ignore_ascii_case(word.as_bytes()))
        })
        .min()
        .unwrap_or(0);
    // An ASCII match begins at a UTF-8 boundary even in multilingual content.
    let start = content[..offset].chars().count().saturating_sub(160);
    let mut characters = content.chars().skip(start);
    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(characters.by_ref().take(800));
    if characters.next().is_some() {
        snippet.push('…');
    }
    snippet
}

#[cfg(test)]
mod tests;
