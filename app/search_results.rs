//! Gold: result granularity and bounded conversation ranking across transports.
use crate::provider::Provider;
use crate::storage::SearchHit;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    rmcp::schemars::JsonSchema,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) enrichment: Option<crate::silver::enrichment::Evidence>,
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
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum HitKey {
    Message(EvidenceKey, String),
    Evidence(EvidenceKey),
}

pub(crate) fn hit_key(hit: &SearchHit, unit: SearchUnit) -> HitKey {
    match unit {
        SearchUnit::Messages => HitKey::Message(evidence_key(hit), hit.id.clone()),
        SearchUnit::Threads => HitKey::Evidence(evidence_key(hit)),
    }
}

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
            let clean = crate::silver::dialogue::visible(&hit.role, &hit.content);
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
                .and_then(crate::silver::dialogue::title);
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
    let mut positions = HashMap::new();
    let mut unique = Vec::<SearchHit>::new();
    for hit in hits {
        // Message mode preserves repeated content with different canonical IDs,
        // but copies of the same message on other nodes occupy one result slot.
        let key = hit_key(&hit, unit);
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
    let mut rounds: [Vec<SearchHit>; 6] = Default::default();
    for hit in hits {
        let key = (hit.provider, hit.conversation_id.clone());
        let count = counts.entry(key).or_insert(0);
        if *count < rounds.len() {
            rounds[*count].push(hit);
            *count += 1;
        }
    }
    rounds.into_iter().flatten().collect()
}

/// Lexical evidence remains eligible even outside the model budget. Semantic-only
/// candidates must have been evaluated by the reranker before becoming excerpts.
/// An unscored nearest neighbour is not evidence. Raw logits are not calibrated:
/// negative scores can still describe useful matches. The joint cosine/logit
/// gate rejects weak neighbours while preserving the audited negative-logit
/// paraphrases; these are model-specific heuristics, not confidence probabilities.
fn qualifies_as_evidence(hit: &SearchHit) -> bool {
    hit.lexical_score.is_some()
        || hit.rerank_score.is_some_and(|score| {
            score.is_finite()
                && (score > 0.0
                    || (score > -2.5
                        && hit
                            .semantic_score
                            .is_some_and(|similarity| similarity >= 0.30)))
        })
}

pub(crate) fn collect(
    hits: Vec<SearchHit>,
    unit: SearchUnit,
    limit: usize,
    query: &str,
) -> Results {
    let exact_reference = crate::search_query::has_reference(query);
    if matches!(unit, SearchUnit::Messages) {
        return Results::Messages(
            eligible(hits)
                .into_iter()
                .filter(|hit| exact_reference || qualifies_as_evidence(hit))
                .filter(|hit| crate::search_query::matches_reference(&hit.content, query))
                .take(limit.min(1000))
                .collect(),
        );
    }
    let mut threads = Vec::<ThreadHit>::new();
    let mut groups = HashMap::new();
    for (rank, hit) in deduplicate(hits, unit)
        .into_iter()
        // A literal reference is independently verifiable even outside the
        // bounded model budget; its exact number is checked below.
        .filter(|hit| exact_reference || qualifies_as_evidence(hit))
        .filter(|hit| crate::search_query::matches_reference(&hit.content, query))
        .enumerate()
    {
        let rank = f32::from(u16::try_from(rank).unwrap_or(u16::MAX));
        let key = (hit.provider, hit.conversation_id.clone());
        let (index, best_rank) = *groups.entry(key.clone()).or_insert_with(|| {
            threads.push(ThreadHit {
                conversation_id: hit.conversation_id.clone(),
                provider: hit.provider,
                node_id: hit.node_id.clone(),
                metadata: ThreadMetadata {
                    enrichment: None,
                    title: hit
                        .metadata
                        .title
                        .clone()
                        .or_else(|| crate::silver::dialogue::title(&hit.content)),
                    ..hit.metadata.clone()
                },
                score: 0.0,
                matches: Vec::new(),
            });
            (threads.len() - 1, rank)
        });
        let thread = &mut threads[index];
        if thread.matches.len() == 3 {
            continue;
        }
        // The best evidence dominates: corroboration can consume only half the
        // gap to the next rank, so extra excerpts cannot beat a stronger match.
        if thread.matches.is_empty() {
            thread.score = 1.0 / (61.0 + rank);
        } else {
            let best = thread.score;
            let base = 1.0 / (61.0 + best_rank);
            let budget = (base - 1.0 / (62.0 + best_rank)) * 0.5;
            let weight = if thread.matches.len() == 1 {
                2.0 / 3.0
            } else {
                1.0 / 3.0
            };
            thread.score = best + budget * weight;
        }
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
    evidence_window(content, query, 800)
}

/// Keep the query evidence visible to the 512-token reranker on long messages.
/// Short messages are passed through unchanged; the bounded window also limits
/// inference cost. Canonical message contents and view anchors remain intact.
pub(crate) fn rerank_document<'a>(content: &'a str, query: &str) -> std::borrow::Cow<'a, str> {
    if content.chars().take(2401).count() <= 2400 {
        std::borrow::Cow::Borrowed(content)
    } else {
        std::borrow::Cow::Owned(evidence_window(content, query, 2400))
    }
}

/// Neighbours inform follow-up intent but never become the displayed anchor.
/// Only searchable projections enter inference, with a fixed character budget.
pub(crate) fn contextual_document(
    hit: &SearchHit,
    query: &str,
    messages: &[crate::storage::Message],
) -> String {
    let mut document = format!(
        "Matched message: {}",
        evidence_window(&hit.content, query, 350)
    );
    let Some(anchor) = messages.iter().position(|message| message.id == hit.id) else {
        return document;
    };
    let mut neighbours = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.id != hit.id)
        .map(|(index, message)| {
            (
                index.abs_diff(anchor),
                message,
                crate::silver::dialogue::visible(&message.role, &message.content),
            )
        })
        .filter(|(_, _, text)| !text.trim().is_empty())
        .collect::<Vec<_>>();
    neighbours.sort_by_key(|(distance, _, _)| *distance);
    for (_, message, text) in neighbours.into_iter().take(2) {
        document.push_str("\nNearby ");
        document.push_str(message.role.as_str());
        document.push_str(" message: ");
        document.push_str(&evidence_window(&text, query, 160));
    }
    document
}

pub(crate) fn evidence_window(content: &str, query: &str, length: usize) -> String {
    let offset = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 3 && word.is_ascii())
        .filter(|word| !crate::search_query::is_stop_word(word))
        .filter_map(|word| {
            content
                .as_bytes()
                .windows(word.len())
                .position(|window| window.eq_ignore_ascii_case(word.as_bytes()))
        })
        .min()
        .unwrap_or(0);
    // An ASCII match begins at a UTF-8 boundary even in multilingual content.
    let leading_context = (length / 3).min(160);
    let start = content[..offset]
        .chars()
        .count()
        .saturating_sub(leading_context);
    let mut characters = content.chars().skip(start);
    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(characters.by_ref().take(length));
    if characters.next().is_some() {
        snippet.push('…');
    }
    snippet
}

#[cfg(test)]
mod tests;
