//! Graceful and abrupt database/API recovery through typed production HTTP.
mod fixture;

use super::{FIRST, OTHER, SECOND};
use crate::provider::Provider;
use crate::search_filter::Filter;
use crate::shared::protocol::{self, Batch, ConversationRecord, Event, MessageRecord, Start};
use fixture::{Api, Backend, Database};

fn vector() -> Vec<f64> {
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    vector
}
fn snapshot(revision: i64, second: bool) -> Vec<Event> {
    let providers = if second {
        vec![Provider::Hermes]
    } else if revision == 1 {
        vec![Provider::Codex, Provider::ClaudeCode, Provider::Pi]
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
            id: if *provider == Provider::ClaudeCode {
                "claude"
            } else if *provider == Provider::Pi {
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
            projection: if *provider == Provider::Pi {
                "retiredneedle".into()
            } else {
                "restartneedle projection".into()
            },
            created_at: Some(1_700_000_000_000),
            generation: Some(crate::semantic::SHARED_EMBEDDING_GENERATION.into()),
            vector: Some(vector()),
        })
        .collect();
    if !second {
        messages.push(MessageRecord {
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
    vec![
        Event::Start(Start {
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
    ]
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
    for unit in [SearchUnit::Messages, SearchUnit::Threads] {
        let mut input = query(unit);
        input.filter = Some(Filter::All(vec![
            Filter::Node(vec!["first".into()]),
            Filter::Provider(vec![Provider::Codex]),
            Filter::Conversation(vec!["codex".into()]),
        ]));
        let result: protocol::Candidates = api.post_json("search", FIRST, &input).await;
        for hits in [result.lexical, result.semantic] {
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].provider, Provider::Codex);
            assert_eq!(hits[0].conversation_id, "codex");
            assert_eq!(hits[0].node_id.as_deref(), Some("first"));
        }
        input.filter = None;
        input.exclude_self = true;
        let result: protocol::Candidates = api.post_json("search", FIRST, &input).await;
        for hits in [result.lexical, result.semantic] {
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].provider, Provider::Hermes);
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

async fn verify(api: &Api, revision: i64) -> String {
    for (token, expected) in [
        (FIRST, (2, 3, 2, 2)),
        (SECOND, (1, 1, 1, 1)),
        (OTHER, (0, 0, 0, 0)),
    ] {
        let counts: crate::storage::Counts = api.get("status", token).await;
        assert_eq!(
            (
                counts.conversations,
                counts.messages,
                counts.searchable_messages,
                counts.embeddings
            ),
            expected
        );
    }
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
    let request = crate::thread_catalog::Request {
        provider: vec![Provider::Codex],
        node: vec!["first".into()],
        title_contains: Some(format!("Current thread {revision}")),
        after: Some(1_699_999_999),
        before: Some(1_700_000_001),
        ..Default::default()
    };
    let page: crate::thread_catalog::Page = api.post_json("threads", FIRST, &request).await;
    assert_eq!(page.threads.len(), 1);
    assert_eq!(page.threads[0].conversation_id, "codex");
    assert_eq!(
        page.threads[0].metadata.project.as_deref(),
        Some("/work/restart")
    );
    assert_eq!(
        page.threads[0].metadata.repository.as_deref(),
        Some("repo://restart")
    );
    let other: crate::thread_catalog::Page = api.post_json("threads", OTHER, &request).await;
    assert!(other.threads.is_empty());
    verify_search(api).await;
    let freshness: crate::shared::Freshness = api.get("freshness", FIRST).await;
    assert_eq!(freshness.nodes.len(), 2);
    assert_eq!(freshness.nodes[0].revision, revision);
    assert_eq!(freshness.nodes[1].revision, 1);
    serde_json::to_string(&freshness).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker, cached backend images, and MASH_TEST_SERVER_BINARY"]
async fn shared_backend_persisted_restart_contract() {
    for backend in [Backend::Postgres, Backend::Redis, Backend::Qdrant] {
        let started = std::time::Instant::now();
        let database = Database::start(backend);
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
        assert_eq!(
            verify(&api, 2).await,
            before,
            "persisted freshness {backend:?}"
        );
        let stale = api.upload(FIRST, &snapshot(1, false)).await;
        assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
        assert_eq!(
            verify(&api, 2).await,
            before,
            "stale update changed {backend:?}"
        );
        publish(&api, FIRST, 3, false).await;
        let before_crash = verify(&api, 3).await;
        // Keep the API alive during the database crash, then replace it too.
        // SIGKILL bypasses database shutdown flushing but preserves host storage.
        database.crash_and_restart().await;
        drop(api);
        let api = Api::start(&database, root.path());
        assert_eq!(
            verify(&api, 3).await,
            before_crash,
            "persisted freshness after process crash {backend:?}"
        );
        let stale = api.upload(FIRST, &snapshot(2, false)).await;
        assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
        assert_eq!(
            verify(&api, 3).await,
            before_crash,
            "stale update after process crash changed {backend:?}"
        );
        publish(&api, FIRST, 4, false).await;
        verify(&api, 4).await;
        drop(api);
        drop(database);
        eprintln!(
            "{backend:?} graceful restart and process crash verified in {:?}",
            started.elapsed()
        );
    }
}
