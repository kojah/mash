//! Gold: result granularity and bounded conversation ranking across transports.
use crate::storage::SearchHit;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub(crate) mod lookup;

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
    Chats,
    Messages,
}

impl SearchUnit {
    pub(crate) const fn candidate_limit(self) -> usize {
        match self {
            Self::Chats => 200,
            Self::Messages => 50,
        }
    }
    pub(crate) const fn rerank_limit(self) -> usize {
        match self {
            Self::Chats => 30,
            Self::Messages => 10,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct ChatMetadata {
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
    Chats(Vec<ChatHit>),
    Messages(Vec<SearchHit>),
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ChatHit {
    #[serde(rename = "chat_id")]
    pub(crate) conversation_id: String,
    pub(crate) source: crate::source::Source,
    pub(crate) kind: crate::source::Kind,
    pub(crate) node_id: Option<String>,
    #[serde(flatten)]
    pub(crate) metadata: ChatMetadata,
    pub(crate) score: f32,
    /// Up to three distinct matching excerpts, ordered by relevance.
    pub(crate) matches: Vec<ChatMatch>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ChatMatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lookup: Option<lookup::Lookup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lexical_match: Option<crate::gold::matching::Evidence>,
    pub(crate) message_id: String,
    pub(crate) node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<crate::role::Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) author_kind: Option<crate::connectors::AuthorKind>,
    pub(crate) created_at: Option<i64>,
    pub(crate) snippet: String,
    /// Primary message's search projection; excludes display ellipses.
    pub(crate) projection_span: ProjectionSpan,
    pub(crate) rerank_score: Option<f32>,
    pub(crate) fusion_score: f32,
    /// Other indexed copies of this evidence, usable as view anchors.
    pub(crate) copies: Vec<MessageReference>,
}

/// Half-open Unicode-scalar character offsets, not bytes or canonical offsets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct ProjectionSpan {
    pub(crate) start_char: usize,
    pub(crate) end_char: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct MessageReference {
    pub(crate) message_id: String,
    pub(crate) node_id: Option<String>,
}

/// Who produced the evidence: a native role or a connector author kind.
pub(crate) type Speaker = (
    Option<crate::role::Role>,
    Option<crate::connectors::AuthorKind>,
);
pub(crate) type EvidenceKey = (crate::source::Source, String, Speaker, blake3::Hash);
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum HitKey {
    Message(EvidenceKey, String),
    Evidence(EvidenceKey),
}

pub(crate) fn hit_key(hit: &SearchHit, unit: SearchUnit) -> HitKey {
    match unit {
        SearchUnit::Messages => HitKey::Message(evidence_key(hit), hit.id.clone()),
        SearchUnit::Chats => HitKey::Evidence(evidence_key(hit)),
    }
}

pub(crate) fn evidence_key(hit: &SearchHit) -> EvidenceKey {
    let text = hit.content.split_whitespace().collect::<Vec<_>>().join(" ");
    (
        hit.source.clone(),
        hit.conversation_id.clone(),
        (hit.role.clone(), hit.author_kind),
        blake3::hash(text.as_bytes()),
    )
}

/// Apply current exclusions even to snapshots published by older nodes.
pub(crate) fn eligible(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    hits.into_iter()
        .filter_map(|mut hit| {
            // Agent-role cleanup applies to native dialogue only; connector
            // text is already the source's own rendering.
            let clean = match &hit.role {
                Some(role) => crate::silver::dialogue::visible(role, &hit.content),
                None => std::borrow::Cow::Borrowed(hit.content.as_str()),
            };
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
        let key = (hit.source.clone(), hit.conversation_id.clone());
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
    collect_inner(hits, unit, limit, query, None)
}

pub(crate) fn collect_exact(
    hits: Vec<SearchHit>,
    unit: SearchUnit,
    limit: usize,
    literal: &crate::gold::exact::Literal,
) -> Results {
    collect_inner(hits, unit, limit, "", Some(literal))
}

/// A chat result seeded from its best hit; matches and score accrue later.
fn chat_for(hit: &SearchHit) -> ChatHit {
    ChatHit {
        conversation_id: hit.conversation_id.clone(),
        source: hit.source.clone(),
        kind: hit.kind,
        node_id: hit.node_id.clone(),
        metadata: ChatMetadata {
            enrichment: None,
            title: hit
                .metadata
                .title
                .clone()
                .or_else(|| crate::silver::dialogue::title(&crate::redaction::text(&hit.content))),
            ..hit.metadata.clone()
        },
        score: 0.0,
        matches: Vec::new(),
    }
}

fn collect_inner(
    hits: Vec<SearchHit>,
    unit: SearchUnit,
    limit: usize,
    query: &str,
    literal: Option<&crate::gold::exact::Literal>,
) -> Results {
    let exact_reference = literal.is_some() || crate::search_query::has_reference(query);
    let matches = |hit: &SearchHit| {
        literal.map_or_else(
            || crate::search_query::matches_reference(&hit.content, query),
            |literal| literal.matches(&hit.content),
        )
    };
    if matches!(unit, SearchUnit::Messages) {
        return Results::Messages(
            eligible(hits)
                .into_iter()
                .filter(|hit| exact_reference || qualifies_as_evidence(hit))
                .filter(&matches)
                .take(limit.min(1000))
                .map(|mut hit| {
                    hit.content = crate::redaction::text(&hit.content);
                    crate::redaction::metadata(&mut hit.metadata);
                    hit
                })
                .collect(),
        );
    }
    let mut threads = Vec::<ChatHit>::new();
    let mut groups = HashMap::new();
    for (rank, mut hit) in deduplicate(hits, unit)
        .into_iter()
        // A literal reference is independently verifiable even outside the
        // bounded model budget; its exact number is checked below.
        .filter(|hit| exact_reference || qualifies_as_evidence(hit))
        .filter(matches)
        .enumerate()
    {
        crate::redaction::metadata(&mut hit.metadata);
        let rank = f32::from(u16::try_from(rank).unwrap_or(u16::MAX));
        let key = (hit.source.clone(), hit.conversation_id.clone());
        let (index, best_rank) = *groups.entry(key.clone()).or_insert_with(|| {
            threads.push(chat_for(&hit));
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
        let (snippet, projection_span) = literal.map_or_else(
            || evidence_excerpt(&hit.content, query, 800),
            |literal| {
                evidence_window_at(
                    &hit.content,
                    literal.find(&hit.content).map_or(0, |range| range.start),
                    800,
                )
            },
        );
        thread.matches.push(ChatMatch {
            lookup: None,
            lexical_match: hit.lexical_match,
            message_id: hit.id,
            node_id: hit.node_id,
            role: hit.role,
            author_kind: hit.author_kind,
            created_at: hit.created_at,
            snippet,
            projection_span,
            rerank_score: hit.rerank_score,
            fusion_score: hit.fusion_score,
            copies: hit.copies,
        });
    }
    threads.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.source.to_string().cmp(&b.source.to_string()))
            .then_with(|| a.conversation_id.cmp(&b.conversation_id))
    });
    threads.truncate(limit.min(1000));
    Results::Chats(threads)
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
                match &message.role {
                    Some(role) => crate::silver::dialogue::visible(role, &message.content),
                    None => std::borrow::Cow::Borrowed(message.content.as_str()),
                },
            )
        })
        .filter(|(_, _, text)| !text.trim().is_empty())
        .collect::<Vec<_>>();
    neighbours.sort_by_key(|(distance, _, _)| *distance);
    for (_, message, text) in neighbours.into_iter().take(2) {
        document.push_str("\nNearby ");
        document.push_str(crate::source::speaker(
            crate::source::Kind::Message,
            message.role.as_ref(),
            message.author_kind,
        ));
        document.push_str(" message: ");
        document.push_str(&evidence_window(&text, query, 160));
    }
    document
}

pub(crate) fn evidence_window(content: &str, query: &str, length: usize) -> String {
    evidence_excerpt(content, query, length).0
}

/// Prefer lexical evidence; semantic-only matches use the projection's start.
fn evidence_excerpt(content: &str, query: &str, length: usize) -> (String, ProjectionSpan) {
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
    evidence_window_at(content, offset, length)
}

fn evidence_window_at(content: &str, offset: usize, length: usize) -> (String, ProjectionSpan) {
    let safe = crate::redaction::projection(content);
    let content = safe.as_str();
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
    let mut end = start;
    for character in characters.by_ref().take(length) {
        snippet.push(character);
        end += 1;
    }
    if characters.next().is_some() {
        snippet.push('…');
    }
    (
        snippet,
        ProjectionSpan {
            start_char: start,
            end_char: end,
        },
    )
}

#[cfg(test)]
mod tests;
