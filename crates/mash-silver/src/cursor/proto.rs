//! Native Agent v1 fields verified against the pinned first-party descriptors.
//! Unrelated operational fields are skipped by prost rather than guessed.
use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub struct Root {
    #[prost(bytes = "vec", repeated, tag = "1")]
    pub messages: Vec<Vec<u8>>,
    #[prost(bytes = "vec", repeated, tag = "8")]
    pub turns: Vec<Vec<u8>>,
    #[prost(string, repeated, tag = "9")]
    pub workspaces: Vec<String>,
    #[prost(bytes = "vec", repeated, tag = "13")]
    pub archives: Vec<Vec<u8>>,
    #[prost(message, repeated, tag = "21")]
    pub repositories: Vec<Repository>,
    #[prost(uint64, optional, tag = "26")]
    pub started_at: Option<u64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Repository {
    #[prost(string, tag = "1")]
    pub path: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Archive {
    #[prost(bytes = "vec", repeated, tag = "1")]
    pub messages: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Turn {
    #[prost(oneof = "TurnKind", tags = "1, 2")]
    pub kind: Option<TurnKind>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
pub enum TurnKind {
    #[prost(message, tag = "1")]
    Agent(AgentTurn),
    #[prost(bytes, tag = "2")]
    Shell(Vec<u8>),
}

#[derive(Clone, PartialEq, Message)]
pub struct AgentTurn {
    #[prost(bytes = "vec", tag = "1")]
    pub user: Vec<u8>,
    #[prost(bytes = "vec", repeated, tag = "2")]
    pub steps: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub struct User {
    #[prost(string, tag = "1")]
    pub text: String,
    #[prost(string, tag = "2")]
    pub id: String,
    #[prost(bool, optional, tag = "5")]
    pub simulated: Option<bool>,
    #[prost(int32, optional, tag = "9")]
    pub simulated_reason: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "18")]
    pub text_blob: Option<Vec<u8>>,
    #[prost(uint64, optional, tag = "25")]
    pub started_at: Option<u64>,
    #[prost(uint64, optional, tag = "26")]
    pub completed_at: Option<u64>,
    #[prost(string, optional, tag = "27")]
    pub sent_by_agent: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Step {
    #[prost(oneof = "StepKind", tags = "1, 2, 3")]
    pub kind: Option<StepKind>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
pub enum StepKind {
    #[prost(message, tag = "1")]
    Assistant(Assistant),
    #[prost(bytes, tag = "2")]
    Tool(Vec<u8>),
    #[prost(bytes, tag = "3")]
    Thinking(Vec<u8>),
}

#[derive(Clone, PartialEq, Message)]
pub struct Assistant {
    #[prost(string, tag = "1")]
    pub text: String,
    #[prost(uint64, optional, tag = "2")]
    pub started_at: Option<u64>,
    #[prost(uint64, optional, tag = "3")]
    pub completed_at: Option<u64>,
}
