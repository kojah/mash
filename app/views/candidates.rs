//! What a search returns before ranking: hits, and the documents that carry their text once.
use crate::storage::SearchHit;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Candidates {
    pub lexical: Vec<SearchHit>,
    pub semantic: Vec<SearchHit>,
    pub documents: Vec<Document>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Document {
    pub node_id: String,
    pub id: String,
    pub projection: String,
}
