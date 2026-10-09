//! Graceful and abrupt database/API recovery through typed production HTTP.
mod fixture;

use super::{FIRST, OTHER, SECOND};
use crate::provider::Provider;
use crate::search_filter::Filter;
use crate::shared::protocol::{self, Batch, ConversationRecord, Event, MessageRecord, Start};
use fixture::{Api, Database};

fn vector() -> Vec<f64> {
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    vector
}
fn source_certificate(revision: i64) -> crate::silver::certificate::Certificate {
    crate::silver::certificate::Certificate {
        revision: u64::try_from(revision).unwrap() * 10,
        interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
        completeness: crate::silver::certificate::Completeness::Complete,
        rejected_sources: 0,
    }
}
fn stored_vector() -> Vec<f64> {
    let mut vector = vector();
    vector[1] = 127.0;
    vector
}
fn snapshot(revision: i64, second: bool) -> Vec<Event> {
    let providers = if second {
        vec![Provider::Opencode]
    } else if revision == 1 {
        vec![Provider::Codex, Provider::ClaudeCode, Provider::Cursor]
    } else {
        vec![Provider::Codex, Provider::ClaudeCode]
    };
    let conversations = providers
        .iter()
        .map(|provider| ConversationRecord {
            enrichment: None,
            id: provider.as_str().into(),
            provider: *provider,
            title: Some(format!("Current thread {revision}")),
            project: Some("/work/restart".into()),
            repository: Some("repo://restart".into()),
            created_at: Some(1_700_000_000_000),
            updated_at: Some(1_700_000_000_000),
        })
        .collect();
    let mut messages: Vec<_> = providers
        .iter()
        .map(|provider| MessageRecord {
            attachments: Vec::new(),
            id: if *provider == Provider::ClaudeCode {
                "claude"
            } else if *provider == Provider::Cursor {
                "retired"
            } else {
                "anchor"
            }
            .into(),
            conversation_id: provider.as_str().into(),
            ordinal: 0,
            context: crate::message_context::MessageContext::Linear,
            role: crate::role::Role::User,
            content: format!("restartneedle canonical revision {revision}"),
            projection: if *provider == Provider::Cursor {
                "retiredneedle".into()
            } else {
                "restartneedle projection".into()
            },
            created_at: Some(1_700_000_000_000),
            generation: Some(crate::semantic::SHARED_EMBEDDING_GENERATION.into()),
            // Off the query axis, so restarts must preserve these exact vectors.
            vector: Some(stored_vector()),
        })
        .collect();
    if !second {
        messages.push(MessageRecord {
            attachments: Vec::new(),
            id: "context".into(),
            conversation_id: "codex".into(),
            ordinal: 1,
            context: crate::message_context::MessageContext::Linear,
            role: crate::role::Role::Tool,
            content: "canonical context-only tool output".into(),
            projection: String::new(),
            created_at: Some(1_700_000_000_000),
            generation: None,
            vector: None,
        });
    }
    let mut events = vec![
        Event::Start(Start {
            gold_build: None,
            source_silver: Some(source_certificate(revision)),
            instance: "restart-instance".into(),
            revision: Some(revision),
            base_revision: None,
            snapshot: true,
            forgets: Vec::new(),
        }),
        Event::Batch(Batch::Conversations {
            records: conversations,
        }),
        Event::Batch(Batch::Messages { records: messages }),
        Event::Finish,
    ];
    if second {
        let Event::Start(start) = &mut events[0] else {
            panic!("start fixture")
        };
        start.snapshot = false;
        start.base_revision = Some(0);
        start.source_silver = None;
    }
    events
}

async fn publish(api: &Api, token: &str, revision: i64, second: bool) {
    let result: protocol::Indexed = api
        .upload(token, &snapshot(revision, second))
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result.revision, revision);
    assert_eq!(
        result.counts.messages,
        if second {
            1
        } else if revision == 1 {
            4
        } else {
            3
        }
    );
}

fn query(unit: crate::search_results::SearchUnit) -> protocol::Search {
    protocol::Search {
        intent: crate::gold::exact::Intent::RelatedDiscussion,
        match_mode: crate::gold::matching::Mode::Strict,
        vector_policy: None,
        unit,
        query: "restartneedle".into(),
        generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
        vector: vector(),
        provider: None,
        days: None,
        exclude_self: false,
        filter: None,
    }
}

async fn verify_search(api: &Api) {
    use crate::search_results::SearchUnit;
    for unit in [SearchUnit::Messages, SearchUnit::Chats] {
        let mut input = query(unit);
        input.filter = Some(Filter::All(vec![
            Filter::Node(vec!["first".into()]),
            Filter::Provider(vec![Provider::Codex]),
            Filter::Chat(vec!["codex".into()]),
        ]));
        let result: protocol::Candidates = api.post_json("search", FIRST, &input).await;
        for hits in [result.lexical, result.semantic] {
            assert_eq!(hits.len(), 1);
            assert_eq!(
                hits[0].source,
                crate::source::Source::Provider(Provider::Codex)
            );
            assert_eq!(hits[0].conversation_id, "codex");
            assert_eq!(hits[0].node_id.as_deref(), Some("first"));
        }
        input.filter = None;
        input.exclude_self = true;
        let result: protocol::Candidates = api.post_json("search", FIRST, &input).await;
        for hits in [result.lexical, result.semantic] {
            assert_eq!(hits.len(), 1);
            assert_eq!(
                hits[0].source,
                crate::source::Source::Provider(Provider::Opencode)
            );
            assert_eq!(hits[0].node_id.as_deref(), Some("second"));
        }
        input.exclude_self = false;
        let result: protocol::Candidates = api.post_json("search", OTHER, &input).await;
        assert!(
            result.lexical.is_empty() && result.semantic.is_empty() && result.documents.is_empty()
        );
    }
    let mut retired = query(SearchUnit::Messages);
    retired.query = "retiredneedle".into();
    let result: protocol::Candidates = api.post_json("search", FIRST, &retired).await;
    assert!(result.lexical.is_empty());
    assert!(result.semantic.iter().all(|hit| hit.id != "retired"));
}

async fn verify_gold(api: &Api) {
    let mut input = query(crate::search_results::SearchUnit::Messages);
    input.filter = Some(Filter::All(vec![
        Filter::Node(vec!["first".into()]),
        Filter::Provider(vec![Provider::Codex]),
    ]));
    let result: protocol::Candidates = api.post_json("search", FIRST, &input).await;
    assert_eq!(result.semantic.len(), 1);
    let score = result.semantic[0].semantic_score.unwrap();
    assert!(
        (0.70..0.72).contains(&score),
        "published vector must survive restart: {score}"
    );
    input.intent = crate::gold::exact::Intent::ExactMention;
    input.generation.clear();
    input.vector.clear();
    let exact: protocol::Candidates = api.post_json("search", FIRST, &input).await;
    assert_eq!(exact.lexical.len(), 1);
    assert!(exact.semantic.is_empty() && exact.documents.is_empty());
}

/// A published node's conversation and message counts; its gold must be current.
async fn node_counts(api: &Api, token: &str, node: &str) -> Option<(i64, i64)> {
    let freshness: crate::shared::Freshness = api.get("freshness", token).await;
    let node = freshness
        .nodes
        .into_iter()
        .find(|entry| entry.node_id == node)?;
    assert!(node.gold_current && node.lexical_current);
    Some((node.conversations, node.messages))
}

async fn verify(api: &Api, revision: i64) -> String {
    let first: crate::shared::Freshness = api.get("freshness", FIRST).await;
    assert_eq!(
        first.nodes[0].source_silver,
        Some(source_certificate(revision))
    );
    assert_eq!(node_counts(api, FIRST, "first").await, Some((2, 3)));
    assert_eq!(node_counts(api, SECOND, "second").await, Some((1, 1)));
    assert_eq!(node_counts(api, OTHER, "first").await, None);
    let input = protocol::View {
        node_id: "first".into(),
        id: "anchor".into(),
        context: 1,
        content: crate::storage::MessageContent::Canonical,
    };
    let messages: Vec<crate::storage::Message> = api.post_json("view", FIRST, &input).await;
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[0].content,
        format!("restartneedle canonical revision {revision}")
    );
    assert_eq!(messages[1].content, "canonical context-only tool output");
    let request = crate::chat_catalog::Request {
        provider: vec![Provider::Codex],
        node: vec!["first".into()],
        title_contains: Some(format!("Current thread {revision}")),
        after: Some(1_699_999_999),
        before: Some(1_700_000_001),
        ..Default::default()
    };
    let page: crate::chat_catalog::Page = api.post_json("chats", FIRST, &request).await;
    assert_eq!(page.chats.len(), 1);
    assert_eq!(page.chats[0].conversation_id, "codex");
    assert_eq!(
        page.chats[0].metadata.project.as_deref(),
        Some("/work/restart")
    );
    assert_eq!(
        page.chats[0].metadata.repository.as_deref(),
        Some("repo://restart")
    );
    let other: crate::chat_catalog::Page = api.post_json("chats", OTHER, &request).await;
    assert!(other.chats.is_empty());
    verify_search(api).await;
    let freshness: crate::shared::Freshness = api.get("freshness", FIRST).await;
    assert_eq!(freshness.nodes.len(), 2);
    assert_eq!(freshness.nodes[0].revision, revision);
    assert_eq!(freshness.nodes[1].revision, 1);
    for node in &freshness.nodes {
        assert!(node.gold_current);
        assert!(node.lexical_current);
        assert!(
            node.lexical_manifest
                .as_ref()
                .unwrap()
                .matches(node.silver_revision.as_ref())
        );
        let manifest = node.gold_manifest.as_ref().unwrap();
        assert_eq!(manifest.build, crate::gold::config::Build::default());
        assert_eq!(Some(&manifest.silver), node.silver_revision.as_ref());
    }
    assert_eq!(
        freshness.nodes[0]
            .silver_revision
            .as_ref()
            .unwrap()
            .revision,
        u64::try_from(revision).unwrap()
    );
    assert_eq!(
        freshness.nodes[1]
            .silver_revision
            .as_ref()
            .unwrap()
            .revision,
        1
    );
    serde_json::to_string(&freshness).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker, cached Qdrant image, and MASH_TEST_SERVER_BINARY"]
async fn shared_backend_snapshot_restore_preserves_silver_gold_and_revision_guards() {
    let database = Database::start();
    database.ready().await;
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(&database, root.path());
    publish(&api, FIRST, 1, false).await;
    publish(&api, SECOND, 1, true).await;
    publish(&api, FIRST, 2, false).await;
    let before = verify(&api, 2).await;
    drop(api);
    let backups = database.backup(root.path()).await;
    assert_eq!(
        backups.len(),
        4,
        "both collections for each configured group"
    );
    // Destroy the original database and its host volume before restoration.
    // The downloaded snapshots are now the only source of the acknowledged data.
    database.destroy();
    let restored = Database::start();
    restored.ready().await;
    restored.restore(&backups).await;
    let api = Api::start(&restored, root.path());
    assert_eq!(verify(&api, 2).await, before);
    verify_gold(&api).await;
    assert_eq!(
        api.upload(FIRST, &snapshot(1, false)).await.status(),
        reqwest::StatusCode::CONFLICT
    );
    assert_eq!(verify(&api, 2).await, before);
    publish(&api, FIRST, 3, false).await;
    verify(&api, 3).await;
    verify_gold(&api).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker, cached Qdrant image, and MASH_TEST_SERVER_BINARY"]
async fn shared_backend_persisted_restart_contract() {
    let started = std::time::Instant::now();
    let database = Database::start();
    database.ready().await;
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(&database, root.path());
    publish(&api, FIRST, 1, false).await;
    publish(&api, SECOND, 1, true).await;
    publish(&api, FIRST, 2, false).await;
    let before = verify(&api, 2).await;
    drop(api);
    database.restart().await;
    let api = Api::start(&database, root.path());
    assert_eq!(verify(&api, 2).await, before, "persisted freshness Qdrant");
    verify_gold(&api).await;
    let stale = api.upload(FIRST, &snapshot(1, false)).await;
    assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(verify(&api, 2).await, before, "stale update changed Qdrant");
    publish(&api, FIRST, 3, false).await;
    let before_crash = verify(&api, 3).await;
    // Keep the API alive during the database crash, then replace it too.
    // SIGKILL bypasses database shutdown flushing but preserves host storage.
    database.crash_and_restart().await;
    drop(api);
    let api = Api::start(&database, root.path());
    verify_gold(&api).await;
    assert_eq!(
        verify(&api, 3).await,
        before_crash,
        "persisted freshness after process crash Qdrant"
    );
    let stale = api.upload(FIRST, &snapshot(2, false)).await;
    assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(
        verify(&api, 3).await,
        before_crash,
        "stale update after process crash changed Qdrant"
    );
    publish(&api, FIRST, 4, false).await;
    verify(&api, 4).await;
    drop(api);
    drop(database);
    eprintln!(
        "Qdrant graceful restart and process crash verified in {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker, cached Qdrant image, MASH_TEST_SERVER_BINARY and MASH_TEST_MODELS_DIR"]
async fn shared_gold_lost_without_backup_is_replayed_from_shared_silver() {
    let database = Database::start();
    database.ready().await;
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(&database, root.path());
    publish(&api, FIRST, 1, false).await;
    publish(&api, SECOND, 1, true).await;
    publish(&api, FIRST, 2, false).await;
    let published: crate::shared::Freshness = api.get("freshness", FIRST).await;
    drop(api);
    // No Qdrant backup: the database and its volume are gone for good.
    database.destroy();
    let fresh = Database::start();
    fresh.ready().await;
    let lost = Api::start(&fresh, root.path());
    let freshness: crate::shared::Freshness = lost.get("freshness", FIRST).await;
    assert!(freshness.nodes.iter().all(|node| !node.gold_current));
    drop(lost);
    let models = std::path::PathBuf::from(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap());
    let (api, reports) = Api::launch(&fresh, root.path(), Some(&models));
    let mut replayed = reports
        .iter()
        .filter(|report| report.get("chats").is_some())
        .map(|report| {
            (
                report["group_id"].as_str().unwrap().to_owned(),
                report["node_id"].as_str().unwrap().to_owned(),
                report["messages"].as_u64().unwrap(),
                report["embeddings"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    replayed.sort();
    assert_eq!(
        replayed,
        [
            ("restart-main".into(), "first".into(), 3, 2),
            ("restart-main".into(), "second".into(), 1, 1),
            ("restart-other".into(), "first".into(), 0, 0),
        ],
        "one replay report per configured node: {reports:?}"
    );
    let replayed: crate::shared::Freshness = api.get("freshness", FIRST).await;
    for (replayed, published) in replayed.nodes.iter().zip(&published.nodes) {
        assert_eq!(replayed.revision, published.revision);
        assert_eq!(replayed.silver_revision, published.silver_revision);
        assert_eq!(replayed.source_silver, published.source_silver);
    }
    assert_eq!(node_counts(&api, FIRST, "first").await, Some((2, 3)));
    assert_eq!(node_counts(&api, SECOND, "second").await, Some((1, 1)));
    let freshness: crate::shared::Freshness = api.get("freshness", FIRST).await;
    assert_eq!(freshness.nodes.len(), 2);
    assert!(
        freshness
            .nodes
            .iter()
            .all(|node| node.gold_current && node.lexical_current)
    );
    let mut input = query(crate::search_results::SearchUnit::Messages);
    input.filter = Some(Filter::All(vec![
        Filter::Node(vec!["first".into()]),
        Filter::Provider(vec![Provider::Codex]),
    ]));
    let result: protocol::Candidates = api.post_json("search", FIRST, &input).await;
    assert_eq!(result.lexical.len(), 1);
    assert_eq!(result.semantic.len(), 1, "replayed vectors are searchable");
    publish(&api, FIRST, 3, false).await;
    assert!(
        api.get::<crate::shared::Freshness>("freshness", FIRST)
            .await
            .nodes[0]
            .gold_current
    );
}
