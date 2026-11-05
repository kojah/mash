use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// Groups results and owns context, such as a chat, channel or thread.
    Container,
    /// A structural part of a container, such as a turn or review: it gives
    /// context and ancestry, but results still group under the container.
    Segment,
    /// Searchable text with an author, such as a message or comment.
    Entry,
    /// Standalone searchable text, such as a note; never a parent.
    Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorKind {
    Human,
    Bot,
    Unknown,
}
