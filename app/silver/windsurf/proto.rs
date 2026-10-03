//! Selected fields from the pinned first-party `CortexTrajectory` descriptors.
pub(super) use super::super::native::{Timestamp, Workspace};
use prost::Message;

#[derive(Clone, Message)]
pub(super) struct Trajectory {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(message, repeated, tag = "2")]
    pub steps: Vec<Step>,
    #[prost(enumeration = "TrajectoryKind", tag = "4")]
    pub kind: i32,
    #[prost(message, optional, tag = "7")]
    pub metadata: Option<TrajectoryMetadata>,
    #[prost(enumeration = "TrajectorySource", tag = "8")]
    pub source: i32,
    #[prost(string, optional, tag = "10")]
    pub title: Option<String>,
}

#[derive(Clone, Message)]
pub(super) struct TrajectoryMetadata {
    #[prost(message, repeated, tag = "1")]
    pub workspaces: Vec<Workspace>,
    #[prost(message, optional, tag = "2")]
    pub created_at: Option<Timestamp>,
}

#[derive(Clone, Message)]
pub(super) struct Step {
    #[prost(enumeration = "StepKind", tag = "1")]
    pub kind: i32,
    #[prost(enumeration = "Status", tag = "4")]
    pub status: i32,
    #[prost(message, optional, tag = "5")]
    pub metadata: Option<StepMetadata>,
    #[prost(message, optional, tag = "19")]
    pub user: Option<UserInput>,
    #[prost(message, optional, tag = "20")]
    pub assistant: Option<PlannerResponse>,
}

#[derive(Clone, Message)]
pub(super) struct StepMetadata {
    #[prost(message, optional, tag = "1")]
    pub created_at: Option<Timestamp>,
    #[prost(enumeration = "StepSource", tag = "3")]
    pub source: i32,
    #[prost(message, optional, tag = "20")]
    pub copied: Option<CopiedStep>,
}

/// Presence establishes copied-step provenance; its opaque references are not prose.
#[derive(Clone, Message)]
pub(super) struct CopiedStep {}

#[derive(Clone, Message)]
pub(super) struct UserInput {
    #[prost(string, tag = "1")]
    pub query: String,
    #[prost(string, tag = "2")]
    pub response: String,
}

#[derive(Clone, Message)]
pub(super) struct PlannerResponse {
    #[prost(string, tag = "1")]
    pub response: String,
    #[prost(string, tag = "3")]
    pub thinking: String,
    #[prost(string, tag = "8")]
    pub modified_response: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub(super) enum TrajectoryKind {
    Unspecified = 0,
    Cascade = 4,
    InteractiveCascade = 17,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub(super) enum TrajectorySource {
    Unspecified = 0,
    CascadeClient = 1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub(super) enum StepKind {
    Unspecified = 0,
    UserInput = 14,
    PlannerResponse = 15,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub(super) enum StepSource {
    Unspecified = 0,
    Manual = 1,
    Model = 2,
    UserImplicit = 3,
    UserExplicit = 4,
    System = 5,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub(super) enum Status {
    Unspecified = 0,
    Pending = 1,
    Running = 2,
    Done = 3,
    Invalid = 4,
    Cleared = 5,
    Canceled = 6,
    Error = 7,
    Generating = 8,
    Waiting = 9,
    Halted = 10,
    Skipping = 11,
}
