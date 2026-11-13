//! A pass that adds a few messages to long chats publishes those messages.
use super::*;
use crate::silver::{Chat, NormalizedMessage, SourceIdentity};
use crate::storage::Storage;
use std::fmt::Write as _;
use std::path::Path;

/// Deterministic, varied words so silver compression and zstd see realistic
/// text instead of one repeated block.
fn text(seed: u64, bytes: usize) -> String {
    const WORDS: [&str; 16] = [
        "index",
        "shared",
        "silver",
        "views",
        "message",
        "publication",
        "chat",
        "vector",
        "upload",
        "server",
        "revision",
        "journal",
        "delta",
        "qdrant",
        "search",
        "history",
    ];
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut output = String::with_capacity(bytes + 16);
    while output.len() < bytes {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        output.push_str(WORDS[usize::try_from(state >> 60).unwrap()]);
        write!(output, "{} ", (state >> 40) % 1000).unwrap();
    }
    output
}

fn message(chat: &str, ordinal: i64, bytes: usize, version: u64) -> NormalizedMessage {
    let seed = blake3::hash(format!("{chat}\0{ordinal}\0{version}").as_bytes());
    NormalizedMessage {
        attachments: Vec::new(),
        id: format!("{chat}-message-{ordinal}"),
        ordinal,
        context: crate::message_context::MessageContext::Linear,
        // Alternate roles, as in agent chats.
        role: if ordinal % 2 == 0 {
            crate::role::Role::User
        } else {
            crate::role::Role::Assistant
        },
        content: text(
            u64::from_le_bytes(seed.as_bytes()[..8].try_into().unwrap()),
            bytes,
        ),
        dialogue_text: None,
        created_at: Some(ordinal),
        tools: crate::tools::Activity::default(),
    }
}

fn chat(id: &str, title: &str, messages: Vec<NormalizedMessage>) -> Chat {
    Chat {
        id: id.into(),
        provider: crate::provider::Provider::ClaudeCode,
        title_origin: None,
        timestamp_origins: None,
        claude_lineage: None,
        source: SourceIdentity::file(format!("/nonexistent/mash-{id}.jsonl")),
        project: Some("/work/mash".into()),
        repository: None,
        title: Some(title.into()),
        created_at: Some(1),
        updated_at: Some(i64::try_from(messages.len()).unwrap()),
        messages: messages.into(),
    }
}

/// Write chats as one complete ingestion pass, like `mash index`.
fn ingest(database: &Path, chats: &[Chat]) {
    let mut writer = Storage::open_writer(database).unwrap();
    for provider in crate::provider::Provider::ALL {
        writer.begin_silver_scan(*provider).unwrap();
    }
    for chat in chats {
        writer.replace_chat(chat).unwrap();
    }
    for provider in crate::provider::Provider::ALL {
        writer.finish_silver_scan(*provider, true).unwrap();
    }
    writer.finish_silver_ingestion().unwrap();
    writer.commit_writer().unwrap();
}

fn environment(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

struct Node {
    config: crate::config::SharedConfig,
    service: Service,
}

impl Node {
    /// The token lives in an existing environment variable so the production
    /// publisher reads it without this test changing process state.
    async fn new() -> Self {
        let token = std::env::var("PATH").unwrap();
        let service = Service::with_parameters(true, 384, 1 << 30, &token).await;
        Self {
            config: crate::config::SharedConfig {
                group_id: service.first.group_id.clone(),
                node_id: service.first.node_id.clone(),
                api_url: service.url.clone(),
                token_env: "PATH".into(),
                publication_redaction: crate::redaction::Policy::ON,
            },
            service,
        }
    }

    async fn publish(
        &self,
        database: &Path,
        fixture: &crate::views::local::test_support::Fixture,
    ) -> (crate::shared::Publication, std::time::Duration) {
        let started = std::time::Instant::now();
        let publication = crate::shared::publish(
            database,
            &self.config,
            Some(crate::shared::Inference {
                models_dir: fixture.model.path(),
                local: Some(&fixture.configuration),
            }),
        )
        .await;
        assert!(!publication.is_pending(), "{:?}", publication.error);
        (publication, started.elapsed())
    }

    async fn node(&self) -> Value {
        let token = std::env::var("PATH").unwrap();
        let freshness = self.service.get("freshness", &token).await;
        freshness["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["node_id"] == "first")
            .cloned()
            .unwrap()
    }
}

/// Search documents Qdrant holds for the service's first node.
async fn documents(service: &Service) -> u64 {
    let response: Value = reqwest::Client::new()
        .post(format!(
            "{}/collections/mash_views_{}_messages/points/count",
            std::env::var("MASH_TEST_QDRANT_URL").unwrap(),
            blake3::hash(service.group.as_bytes()).to_hex()
        ))
        .json(&json!({
            "exact": true,
            "filter": {"must": [{"key": "node_id", "match": {"value": "first"}}]}
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    response["result"]["count"].as_u64().unwrap()
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
#[allow(
    clippy::too_many_lines,
    reason = "one publication history compared with a fresh replacement"
)]
async fn qdrant_long_chat_delta_publishes_only_changed_messages() {
    let length = environment("MASH_TEST_LONG_CHAT_MESSAGES", 300);
    let bytes = environment("MASH_TEST_LONG_CHAT_MESSAGE_BYTES", 2048);
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("mash.sqlite3");
    let original = |id: &str| {
        (0..i64::try_from(length).unwrap())
            .map(|ordinal| message(id, ordinal, bytes, 0))
            .collect::<Vec<_>>()
    };
    ingest(
        &database,
        &[
            chat("first-long", "First long chat", original("first-long")),
            chat("second-long", "Second long chat", original("second-long")),
            chat("short", "Short chat", original("short")[..3].to_vec()),
        ],
    );
    let fixture = crate::views::local::test_support::Fixture::new(&database, "first").await;
    let delta = Node::new().await;
    let (first, elapsed) = delta.publish(&database, &fixture).await;
    eprintln!(
        "initial publication: {elapsed:?} {}",
        serde_json::to_string(&first.preparation).unwrap()
    );

    // One pass: both long chats grow by four messages; the first also edits
    // one message, drops another and is renamed. A new chat appears and the
    // short chat is untouched.
    let grown = |id: &str| {
        let mut messages = original(id);
        let end = i64::try_from(length).unwrap();
        messages.extend((end..end + 4).map(|ordinal| message(id, ordinal, bytes, 0)));
        messages
    };
    let mut first_long = grown("first-long");
    first_long[2] = message("first-long", 2, bytes, 1);
    first_long.remove(5);
    ingest(
        &database,
        &[
            chat("first-long", "First long chat, renamed", first_long),
            chat("second-long", "Second long chat", grown("second-long")),
            chat("short", "Short chat", original("short")[..3].to_vec()),
            chat("added", "Added chat", original("added")[..2].to_vec()),
        ],
    );
    crate::views::local::build(
        &database,
        &directory.path().join("no-models"),
        &fixture.configuration,
        false,
    )
    .await
    .unwrap();
    let (second, elapsed) = delta.publish(&database, &fixture).await;
    let work = serde_json::to_value(&second.preparation).unwrap();
    eprintln!(
        "delta publication of {length} x {bytes} B chats: {elapsed:?} {}",
        serde_json::to_string(&work).unwrap()
    );
    // Eleven new or edited messages carry vectors: four new in each long
    // chat, the edit and the added chat's two. The upload holds those
    // messages, not the long chats.
    assert_eq!(
        work["forwarded_embeddings"].as_u64().unwrap() + work["model_inferences"].as_u64().unwrap(),
        11,
        "{work}"
    );
    assert!(
        work["upload_bytes"].as_u64().unwrap() < u64::try_from(bytes * 64).unwrap(),
        "{work}"
    );
    let published = delta.node().await;
    assert_eq!(published["views_current"], true, "{published:#}");

    // A fresh service given the same history as one replacement must hold the
    // same canonical silver and the same searchable documents.
    let replacement = Node::new().await;
    replacement.publish(&database, &fixture).await;
    let expected = replacement.node().await;
    assert_eq!(
        published["silver_revision"]["fingerprint"], expected["silver_revision"]["fingerprint"],
        "{published:#}\n{expected:#}"
    );
    let counts = |node: &Node| {
        let identity = node.service.first.clone();
        let store = node.service.store.clone();
        async move { store.status(&identity).await.unwrap() }
    };
    let (delta_counts, replacement_counts) = (counts(&delta).await, counts(&replacement).await);
    assert_eq!(
        serde_json::to_value(&delta_counts).unwrap(),
        serde_json::to_value(&replacement_counts).unwrap()
    );
    // Every message has a nonempty projection, so each has one document.
    for node in [&delta, &replacement] {
        assert_eq!(documents(&node.service).await, delta_counts.messages);
    }
    fixture.cleanup().await;
    delta.service.cleanup().await;
    replacement.service.cleanup().await;
}
