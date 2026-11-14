//! Built-in views: result granularity and bounded chat ranking across transports.
use crate::AppError;
use crate::storage::SearchHit;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod lookup;

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
pub enum SearchUnit {
    #[default]
    Chats,
    Messages,
}

impl SearchUnit {
    pub const fn candidate_limit(self) -> usize {
        match self {
            Self::Chats => 200,
            Self::Messages => 50,
        }
    }
    pub const fn rerank_limit(self) -> usize {
        match self {
            Self::Chats => 30,
            Self::Messages => 10,
        }
    }
    /// The final ranked list is cut to this many results; pages slice it.
    pub const fn result_window(self) -> usize {
        self.candidate_limit()
    }
    const fn name(self) -> &'static str {
        match self {
            Self::Chats => "chats",
            Self::Messages => "messages",
        }
    }
}

/// One page, `[offset, offset + limit)`, of a search's final ranked list.
/// Ranking never depends on the page, so pages partition one ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub offset: usize,
    pub limit: usize,
}

impl Page {
    /// Requests share the `views.query.limit` configuration bound.
    pub const MAX_LIMIT: usize = 200;

    pub fn new(limit: usize, offset: usize, unit: SearchUnit) -> Result<Self, AppError> {
        let window = unit.result_window();
        if !(1..=Self::MAX_LIMIT).contains(&limit) || offset >= window {
            return Err(AppError::usage(format!(
                "search pages require limit 1–{} and offset 0–{} for unit {}",
                Self::MAX_LIMIT,
                window - 1,
                unit.name()
            )));
        }
        Ok(Self { offset, limit })
    }

    /// The first page of `limit` results, for callers outside request validation.
    #[cfg(test)]
    pub const fn first(limit: usize) -> Self {
        Self { offset: 0, limit }
    }

    /// Slice the ranked results; `next_offset` is set only when more remain.
    fn take<T>(self, ranked: impl Iterator<Item = T>, window: usize) -> (Vec<T>, Option<usize>) {
        let mut ranked = ranked.take(window).skip(self.offset);
        let page = ranked.by_ref().take(self.limit).collect();
        let next = self.offset + self.limit;
        (page, ranked.next().is_some().then_some(next))
    }
}

/// A page of results and the offset of the following page, if any.
pub struct Collected {
    pub results: Results,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ChatMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrichment: Option<crate::silver::enrichment::Evidence>,
    pub title: Option<String>,
    pub project: Option<String>,
    pub repository: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl ChatMetadata {
    /// Masks credentials in the descriptive fields; enrichment derived from a
    /// masked field is dropped with it.
    pub fn redact(&mut self, policy: crate::redaction::Policy) {
        if !policy.enabled() {
            return;
        }
        let mut changed = false;
        for value in [&mut self.title, &mut self.project, &mut self.repository]
            .into_iter()
            .flatten()
        {
            let safe = crate::redaction::text(value);
            changed |= safe != *value;
            *value = safe;
        }
        if changed {
            self.enrichment = None;
        }
    }
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Results {
    Chats(Vec<ChatHit>),
    Messages(Vec<SearchHit>),
}

impl Results {
    /// Reorder newest first; undated results keep their relevance order last.
    /// Search applies this to one page, after paging the relevance order.
    pub fn newest_first(&mut self) {
        use std::cmp::Reverse;
        match self {
            Self::Messages(hits) => hits.sort_by_key(|hit| Reverse(hit.created_at)),
            Self::Chats(chats) => chats.sort_by_key(|chat| {
                Reverse(chat.matches.iter().filter_map(|m| m.created_at).max())
            }),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ChatHit {
    pub chat_id: String,
    pub provider: crate::provider::Provider,
    pub node_id: Option<String>,
    #[serde(flatten)]
    pub metadata: ChatMetadata,
    pub score: f32,
    /// Up to three distinct matching excerpts, ordered by relevance.
    pub matches: Vec<ChatMatch>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ChatMatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookup: Option<lookup::Lookup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lexical_match: Option<crate::views::matching::Evidence>,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub node_id: Option<String>,
    pub role: crate::role::Role,
    pub created_at: Option<i64>,
    pub snippet: String,
    /// Primary message's search projection; excludes display ellipses.
    pub projection_span: ProjectionSpan,
    pub rerank_score: Option<f32>,
    pub fusion_score: f32,
    /// Other indexed copies of this evidence, usable as view anchors.
    pub copies: Vec<MessageReference>,
}

/// Half-open Unicode-scalar character offsets, not bytes or canonical offsets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ProjectionSpan {
    pub start_char: usize,
    pub end_char: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub struct MessageReference {
    pub message_id: String,
    pub node_id: Option<String>,
}

pub type EvidenceKey = (
    crate::provider::Provider,
    String,
    crate::role::Role,
    blake3::Hash,
);
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum HitKey {
    Message(EvidenceKey, String),
    Evidence(EvidenceKey),
}

pub fn hit_key(hit: &SearchHit, unit: SearchUnit) -> HitKey {
    match unit {
        SearchUnit::Messages => HitKey::Message(evidence_key(hit), hit.id.clone()),
        SearchUnit::Chats => HitKey::Evidence(evidence_key(hit)),
    }
}

pub fn evidence_key(hit: &SearchHit) -> EvidenceKey {
    let text = hit.content.split_whitespace().collect::<Vec<_>>().join(" ");
    (
        hit.provider,
        hit.chat_id.clone(),
        hit.role.clone(),
        blake3::hash(text.as_bytes()),
    )
}

/// Apply current exclusions even to snapshots published by older nodes.
pub fn eligible(hits: Vec<SearchHit>) -> Vec<SearchHit> {
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

pub fn merge_copies(existing: &mut SearchHit, other: SearchHit) {
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

/// A fork copies its parent's messages into a new chat with their original
/// times. The same words from the same role at the same instant in another
/// chat are that one message again, not more evidence.
fn fork_key(hit: &SearchHit, unit: SearchUnit) -> Option<ForkKey> {
    let HitKey::Evidence((provider, _, role, text)) = hit_key(hit, unit) else {
        return None;
    };
    Some((provider, role, text, hit.created_at?))
}

type ForkKey = (
    crate::provider::Provider,
    crate::role::Role,
    blake3::Hash,
    i64,
);

pub fn deduplicate(hits: Vec<SearchHit>, unit: SearchUnit) -> Vec<SearchHit> {
    let hits = eligible(hits);
    let mut positions = HashMap::new();
    let mut forks = HashMap::new();
    let mut unique = Vec::<SearchHit>::new();
    for mut hit in hits {
        // Message mode preserves repeated content with different canonical IDs,
        // but copies of the same message on other nodes occupy one result slot.
        let key = hit_key(&hit, unit);
        let fork = fork_key(&hit, unit);
        let seen: Option<usize> = positions
            .get(&key)
            .or_else(|| fork.as_ref().and_then(|fork| forks.get(fork)))
            .copied();
        if let Some(index) = seen {
            // One chat stands for all forks, chosen the same way in every
            // candidate list so lexical and semantic evidence still fuse.
            if hit.chat_id < unique[index].chat_id {
                std::mem::swap(&mut unique[index], &mut hit);
                positions.insert(key, index);
            }
            merge_copies(&mut unique[index], hit);
        } else {
            positions.insert(key, unique.len());
            if let Some(fork) = fork {
                forks.insert(fork, unique.len());
            }
            unique.push(hit);
        }
    }
    unique
}

/// Round-robin the best candidates from each chat before bounded reranking.
/// Identical content (including copies on other nodes) earns no additional evidence.
pub fn diversify(hits: Vec<SearchHit>, unit: SearchUnit) -> Vec<SearchHit> {
    let hits = deduplicate(hits, unit);
    if matches!(unit, SearchUnit::Messages) {
        return hits;
    }
    let mut counts = HashMap::new();
    let mut rounds: [Vec<SearchHit>; 6] = Default::default();
    for hit in hits {
        let key = (hit.provider, hit.chat_id.clone());
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

/// `redaction` is the output policy: results leave through CLI or MCP.
pub fn collect(
    hits: Vec<SearchHit>,
    unit: SearchUnit,
    page: Page,
    query: &str,
    redaction: crate::redaction::Policy,
) -> Collected {
    collect_inner(hits, unit, page, query, None, redaction)
}

pub fn collect_exact(
    hits: Vec<SearchHit>,
    unit: SearchUnit,
    page: Page,
    literal: &crate::views::exact::Literal,
    redaction: crate::redaction::Policy,
) -> Collected {
    collect_inner(hits, unit, page, "", Some(literal), redaction)
}

/// A chat result seeded from its best hit; matches and score accrue later.
fn chat_for(hit: &SearchHit, redaction: crate::redaction::Policy) -> ChatHit {
    ChatHit {
        chat_id: hit.chat_id.clone(),
        provider: hit.provider,
        node_id: hit.node_id.clone(),
        metadata: ChatMetadata {
            enrichment: None,
            title: hit
                .metadata
                .title
                .clone()
                .or_else(|| crate::silver::dialogue::title(&redaction.text(&hit.content))),
            ..hit.metadata.clone()
        },
        score: 0.0,
        matches: Vec::new(),
    }
}

fn collect_inner(
    hits: Vec<SearchHit>,
    unit: SearchUnit,
    page: Page,
    query: &str,
    literal: Option<&crate::views::exact::Literal>,
    redaction: crate::redaction::Policy,
) -> Collected {
    let exact_reference = literal.is_some() || crate::search_query::has_reference(query);
    let matches = |hit: &SearchHit| {
        literal.map_or_else(
            || crate::search_query::matches_reference(&hit.content, query),
            |literal| literal.matches(&hit.content),
        )
    };
    if matches!(unit, SearchUnit::Messages) {
        let (mut hits, next_offset) = page.take(
            eligible(hits)
                .into_iter()
                .filter(|hit| exact_reference || qualifies_as_evidence(hit))
                .filter(&matches),
            unit.result_window(),
        );
        for hit in &mut hits {
            hit.content = redaction.text(&hit.content);
            hit.metadata.redact(redaction);
        }
        return Collected {
            results: Results::Messages(hits),
            next_offset,
        };
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
        hit.metadata.redact(redaction);
        let rank = f32::from(u16::try_from(rank).unwrap_or(u16::MAX));
        let key = (hit.provider, hit.chat_id.clone());
        let (index, best_rank) = *groups.entry(key.clone()).or_insert_with(|| {
            threads.push(chat_for(&hit, redaction));
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
            || evidence_excerpt(&hit.content, query, 800, redaction),
            |literal| {
                evidence_window_at(
                    &hit.content,
                    literal.find(&hit.content).map_or(0, |range| range.start),
                    800,
                    redaction,
                )
            },
        );
        thread.matches.push(ChatMatch {
            lookup: None,
            lexical_match: hit.lexical_match,
            message_id: hit.id,
            turn_id: hit.turn_id,
            node_id: hit.node_id,
            role: hit.role.clone(),

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
            .then_with(|| a.provider.to_string().cmp(&b.provider.to_string()))
            .then_with(|| a.chat_id.cmp(&b.chat_id))
    });
    let (threads, next_offset) = page.take(threads.into_iter(), unit.result_window());
    Collected {
        results: Results::Chats(threads),
        next_offset,
    }
}

/// Keep the query evidence visible to the 512-token reranker on long messages.
/// Short messages are passed through unchanged; the bounded window also limits
/// inference cost. Canonical message contents and view anchors remain intact.
pub fn rerank_document<'a>(content: &'a str, query: &str) -> std::borrow::Cow<'a, str> {
    if content.chars().take(2401).count() <= 2400 {
        std::borrow::Cow::Borrowed(content)
    } else {
        std::borrow::Cow::Owned(evidence_window(content, query, 2400))
    }
}

/// Neighbours inform follow-up intent but never become the displayed anchor.
/// Only searchable projections enter inference, with a fixed character budget.
pub fn contextual_document(
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

/// A masked window, for model inputs.
pub fn evidence_window(content: &str, query: &str, length: usize) -> String {
    evidence_window_with(content, query, length, crate::redaction::Policy::ON)
}

pub fn evidence_window_with(
    content: &str,
    query: &str,
    length: usize,
    redaction: crate::redaction::Policy,
) -> String {
    evidence_excerpt(content, query, length, redaction).0
}

/// Prefer lexical evidence; semantic-only matches use the projection's start.
fn evidence_excerpt(
    content: &str,
    query: &str,
    length: usize,
    redaction: crate::redaction::Policy,
) -> (String, ProjectionSpan) {
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
    evidence_window_at(content, offset, length, redaction)
}

fn evidence_window_at(
    content: &str,
    offset: usize,
    length: usize,
    redaction: crate::redaction::Policy,
) -> (String, ProjectionSpan) {
    // Masking before windowing keeps a secret cut by the window from leaking.
    let safe = redaction.projection(content);
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
