//! Gold-only upload events: embeddings for retained, certified shared silver.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Event {
    Start {
        publication_revision: i64,
        silver: super::silver_revision::State,
        generation: String,
        #[serde(default)]
        build: crate::gold::config::Build,
    },
    Embeddings {
        records: Vec<Embedding>,
    },
    Finish,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Embedding {
    pub(super) message_id: String,
    pub(super) vector: Vec<f64>,
}
