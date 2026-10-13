//! Test assets shared by CLI integration and MCP protocol tests.
//! Marker fixtures validate installation metadata without loading real models.
use std::{path::Path, time::Duration};

/// Bound for a wait that only guards against a hung test. Completion or an
/// explicit synchronization signal ends the wait; this is not a latency
/// expectation. Shared build hosts run under heavy load, so a correct run must
/// never race it. Nextest still terminates the complete test after its
/// profile's slow-timeout.
pub(crate) const HANG_GUARD: Duration = Duration::from_secs(60);

pub(crate) fn create_valid_model_marker(root: &Path) {
    std::fs::create_dir_all(root).expect("model root");
    std::fs::write(root.join("dummy-model"), [0_u8]).expect("dummy model asset");
    std::fs::write(
        root.join("installed.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "embedding_model": "AllMiniLML6V2Q",
            "reranker_model": "jinaai/jina-reranker-v1-turbo-en",
            "embedding_dimensions": 384,
            "coreml_embedding": cfg!(all(target_os = "macos", target_arch = "aarch64")),
            "files": [{"path": "dummy-model", "size": 1}]
        }))
        .expect("model marker JSON"),
    )
    .expect("model marker");
}
#[path = "test_support/vector.rs"]
pub(crate) mod vector;
