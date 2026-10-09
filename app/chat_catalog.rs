//! Gold metadata view: bounded thread discovery over canonical silver records.
use crate::AppError;
use crate::provider::Provider;
use crate::search_results::ChatMetadata;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

#[cfg(test)]
mod tests;

#[derive(
    Debug, Clone, Copy, Default, Deserialize, Serialize, clap::ValueEnum, rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sort {
    #[default]
    UpdatedAt,
    CreatedAt,
    Title,
}

#[derive(
    Debug, Clone, Copy, Default, Deserialize, Serialize, clap::ValueEnum, rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    Asc,
    #[default]
    Desc,
}

#[derive(Debug, Clone, Deserialize, Serialize, clap::Args, rmcp::schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Request {
    #[arg(long)]
    pub(crate) provider: Vec<Provider>,
    #[arg(long)]
    pub(crate) node: Vec<String>,
    /// Literal substring, ASCII case insensitive; no wildcard syntax.
    #[arg(long)]
    pub(crate) title_contains: Option<String>,
    /// Inclusive thread activity cutoff in Unix seconds.
    #[arg(long)]
    pub(crate) after: Option<i64>,
    /// Exclusive thread activity cutoff in Unix seconds.
    #[arg(long)]
    pub(crate) before: Option<i64>,
    #[arg(long, value_enum, default_value = "updated-at")]
    pub(crate) sort: Sort,
    #[arg(long, value_enum, default_value = "desc")]
    pub(crate) direction: Direction,
    /// Results per page, from 1 to 100; use `next_offset` to fetch another page.
    #[schemars(range(min = 1, max = 100))]
    #[arg(long, default_value_t = 50)]
    pub(crate) limit: usize,
    /// Page offset, from 0 to 10000.
    #[schemars(range(min = 0, max = 10000))]
    #[arg(long, default_value_t = 0)]
    pub(crate) offset: usize,
    /// Exclude the caller's published copy, for combined local/shared discovery.
    #[arg(skip)]
    pub(crate) exclude_self: bool,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            provider: Vec::new(),
            node: Vec::new(),
            title_contains: None,
            after: None,
            before: None,
            sort: Sort::default(),
            direction: Direction::default(),
            limit: 50,
            offset: 0,
            exclude_self: false,
        }
    }
}

impl Request {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if !(1..=100).contains(&self.limit) || self.offset > 10_000 {
            return Err(AppError::usage(
                "chat pages require limit 1–100 and offset 0–10000",
            ));
        }
        crate::search_filter::Filter::Provider(self.provider.clone()).validate()?;
        crate::search_filter::Filter::Node(self.node.clone()).validate()?;
        if let Some(title) = &self.title_contains
            && (title.trim().is_empty()
                || title.len() > 4096
                || title.chars().any(char::is_control))
        {
            return Err(AppError::usage(
                "chat title filters require nonempty text of at most 4096 bytes without control characters",
            ));
        }
        for timestamp in [self.after, self.before].into_iter().flatten() {
            crate::search_filter::Filter::After(timestamp).validate()?;
        }
        Ok(())
    }

    pub(crate) fn admits_node(&self, node: &str, caller: &str) -> bool {
        !(self.exclude_self && node == caller)
            && (self.node.is_empty() || self.node.iter().any(|value| value == node))
    }

    fn matches(&self, thread: &Chat) -> bool {
        (self.provider.is_empty() || self.provider.contains(&thread.provider))
            && self.title_contains.as_ref().is_none_or(|title| {
                thread.metadata.title.as_ref().is_some_and(|value| {
                    value
                        .to_ascii_lowercase()
                        .contains(&title.to_ascii_lowercase())
                })
            })
            && self.after.is_none_or(|after| {
                thread
                    .metadata
                    .updated_at
                    .is_some_and(|time| time >= after * 1000)
            })
            && self.before.is_none_or(|before| {
                thread
                    .metadata
                    .updated_at
                    .is_some_and(|time| time < before * 1000)
            })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Chat {
    #[serde(rename = "chat_id")]
    pub(crate) conversation_id: String,
    pub(crate) provider: Provider,
    pub(crate) node_id: String,
    #[serde(flatten)]
    pub(crate) metadata: ChatMetadata,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Page {
    pub(crate) chats: Vec<Chat>,
    pub(crate) next_offset: Option<usize>,
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
enum Value {
    Time(i64),
    Title(String),
}

#[derive(Clone, Eq, PartialEq)]
struct Key {
    value: Option<Value>,
    descending: bool,
    provider: Provider,
    id: String,
    node: String,
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        let direction = self.descending.cmp(&other.descending);
        if direction != Ordering::Equal {
            return direction;
        }
        let value = match (&self.value, &other.value) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(left), Some(right)) => {
                let order = left.cmp(right);
                if self.descending {
                    order.reverse()
                } else {
                    order
                }
            }
        };
        value
            .then_with(|| self.provider.as_str().cmp(other.provider.as_str()))
            .then_with(|| self.id.cmp(&other.id))
            .then_with(|| self.node.cmp(&other.node))
    }
}
impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Retain only a bounded page window; duplicate copies choose their best sort key.
pub(crate) struct Collector {
    request: Request,
    rows: BTreeMap<Key, Chat>,
    identities: HashMap<(Provider, String), Key>,
}

impl Collector {
    pub(crate) fn new(request: Request) -> Result<Self, AppError> {
        request.validate()?;
        Ok(Self {
            request,
            rows: BTreeMap::new(),
            identities: HashMap::new(),
        })
    }

    pub(crate) fn push(&mut self, thread: Chat) {
        if !self.request.matches(&thread) {
            return;
        }
        let value = match self.request.sort {
            Sort::UpdatedAt => thread.metadata.updated_at.map(Value::Time),
            Sort::CreatedAt => thread.metadata.created_at.map(Value::Time),
            Sort::Title => thread.metadata.title.clone().map(Value::Title),
        };
        let key = Key {
            value,
            descending: matches!(self.request.direction, Direction::Desc),
            provider: thread.provider,
            id: thread.conversation_id.clone(),
            node: thread.node_id.clone(),
        };
        let identity = (thread.provider, thread.conversation_id.clone());
        if let Some(previous) = self.identities.get(&identity) {
            if key >= *previous {
                return;
            }
            self.rows.remove(previous);
        }
        self.identities.insert(identity, key.clone());
        self.rows.insert(key, thread);
        if self.rows.len() > self.request.offset + self.request.limit + 1 {
            let (_, removed) = self
                .rows
                .pop_last()
                .expect("window contains at least one row");
            self.identities
                .remove(&(removed.provider, removed.conversation_id));
        }
    }

    pub(crate) fn finish(self) -> Page {
        let next_offset = (self.rows.len() > self.request.offset + self.request.limit
            && self.request.offset + self.request.limit <= 10_000)
            .then_some(self.request.offset + self.request.limit);
        Page {
            chats: self
                .rows
                .into_values()
                .skip(self.request.offset)
                .take(self.request.limit)
                .collect(),
            next_offset,
        }
    }
}
