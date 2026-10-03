use super::*;
use crate::{
    gold::exact::Intent,
    provider::Provider,
    search_filter::{Filter, Selection},
    search_results::SearchUnit,
    shared::protocol::{
        Batch, Candidates, ConversationRecord, Event, MessageRecord, Search, Start,
    },
};

fn row(id: &str, ordinal: i64, content: &str, created_at: Option<i64>) -> MessageRecord {
    MessageRecord {
        id: id.into(),
        conversation_id: "conversation".into(),
        ordinal,
        context: crate::message_context::MessageContext::Linear,
        role: crate::role::Role::User,
        content: content.into(),
        projection: content.into(),
        created_at,
        generation: None,
        vector: None,
    }
}

fn records() -> Vec<MessageRecord> {
    let mut rows = (0..1100)
        .map(|i| row(&format!("noise-{i}"), i, "voyage code 4", Some(i)))
        .collect::<Vec<_>>();
    for (id, content, time) in [
        ("old", "use voyage-code-4", Some(1000)),
        ("recent", "use VOYAGE-code-4", Some(2000)),
        ("undated", "use voyage-code-4 without a timestamp", None),
        ("suffix", "voyage-code-40 voyage-code-4-v2", Some(3000)),
        ("cpp", "use C++", Some(1000)),
        ("cpp-decoy", "use C++20", Some(1000)),
        ("overlap", "xfoo foo foo", Some(1000)),
        ("phrase", "connection pool", Some(1000)),
        ("phrase-decoy", "connection  pool", Some(1000)),
        ("for", "for loops in Rust", Some(1000)),
    ] {
        rows.push(row(id, i64::try_from(rows.len()).unwrap(), content, time));
    }
    let mut hidden = row(
        "hidden",
        i64::try_from(rows.len()).unwrap(),
        "hidden-output-marker voyage-code-4",
        Some(4000),
    );
    hidden.role = crate::role::Role::Tool;
    hidden.projection.clear();
    rows.push(hidden);
    // Apply current visibility before the limit even for old tool projections.
    for i in 0..60 {
        let mut tool = row(
            &format!("legacy-tool-{i}"),
            i64::try_from(rows.len()).unwrap(),
            "voyage-code-4",
            Some(10000 + i),
        );
        tool.role = crate::role::Role::Tool;
        rows.push(tool);
    }
    // Genuine matches in another provider must not consume a scoped budget.
    for i in 0..100 {
        let mut other = row(&format!("claude-{i}"), i, "voyage-code-4", Some(4000 + i));
        other.conversation_id = "claude".into();
        rows.push(other);
    }
    rows
}

fn events(revision: i64) -> Vec<Value> {
    let conversation = ConversationRecord {
        enrichment: None,
        id: "conversation".into(),
        provider: Provider::Codex,
        title: Some("title-only-marker".into()),
        project: Some("/work/mash".into()),
        repository: Some("repo://mash".into()),
        created_at: Some(1000),
        updated_at: Some(2000),
    };
    let mut other = conversation.clone();
    other.id = "claude".into();
    other.provider = Provider::ClaudeCode;
    let mut events = vec![
        Event::Start(Start {
            instance: "exact-contract".into(),
            revision: Some(revision),
            base_revision: None,
            snapshot: true,
            forgets: vec![],
            source_silver: Some(crate::silver::certificate::Certificate {
                revision: u64::try_from(revision).unwrap(),
                interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
                completeness: crate::silver::certificate::Completeness::Complete,
                rejected_sources: 0,
            }),
        }),
        Event::Batch(Batch::Conversations {
            records: vec![conversation, other],
        }),
    ];
    for batch in records().chunks(128) {
        events.push(Event::Batch(Batch::Messages {
            records: batch.to_vec(),
        }));
    }
    events.push(Event::Finish);
    events
        .into_iter()
        .map(|event| serde_json::to_value(event).unwrap())
        .collect()
}

fn query(text: &str, filter: Filter, exclude_self: bool) -> Search {
    Search {
        intent: Intent::ExactMention,
        unit: SearchUnit::Messages,
        query: text.into(),
        generation: String::new(),
        vector: vec![],
        provider: None,
        days: None,
        exclude_self,
        filter: Some(filter),
    }
}

async fn search(service: &Service, token: &str, input: &Search) -> Candidates {
    let response = service
        .request("search", token)
        .json(input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_exact_mentions_page_before_limits_preserve_silver_and_merge_without_models() {
    let token = std::env::var("HOME").unwrap();
    let service = Service::with_parameters(true, 384, 64 * 1024 * 1024, &token).await;
    let first = service
        .request("search", &token)
        .json(&query("voyage-code-4", Filter::All(vec![]), false))
        .send()
        .await
        .unwrap();
    assert_eq!(
        first.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "uninitialized index must fail"
    );
    for auth in [token.as_str(), SECOND, OTHER] {
        let response = service.upload(auth, &events(1)).await;
        let status = response.status();
        assert_eq!(status, StatusCode::OK, "{}", response.text().await.unwrap());
    }
    let silver = service.get("silver", &token).await;
    let freshness = service.get("freshness", &token).await;
    assert!(
        freshness["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["lexical_current"] == true && n["gold_current"] == false)
    );
    verify_literals(&service, &token).await;
    verify_scopes(&service, &token).await;
    verify_core(&service).await;
    assert_eq!(service.get("silver", &token).await, silver);
    assert_eq!(service.get("freshness", &token).await, freshness);
    service.cleanup().await;
}

async fn verify_literals(service: &Service, token: &str) {
    let mut timings = Vec::new();
    for (text, per_node) in [
        ("voyage-code-4", 3),
        ("\"voyage-code-4\"", 3),
        ("C++", 1),
        ("foo foo", 1),
        ("connection pool", 1),
        ("for", 1),
        ("hidden-output-marker", 0),
        ("title-only-marker", 0),
    ] {
        let started = std::time::Instant::now();
        let result = search(
            service,
            token,
            &query(text, Filter::Provider(vec![Provider::Codex]), false),
        )
        .await;
        timings.push(started.elapsed().as_millis());
        assert_eq!(result.lexical.len(), per_node * 2, "{text}");
        assert!(result.semantic.is_empty() && result.documents.is_empty());
        assert!(
            result
                .lexical
                .iter()
                .all(|h| h.semantic_score.is_none() && h.rerank_score.is_none())
        );
    }
    eprintln!(
        "Qdrant exact HTTP fixture (1271 messages/node, 2 nodes), query times ms: {timings:?}"
    );
}

async fn verify_scopes(service: &Service, token: &str) {
    let scoped = Selection {
        provider: vec![Provider::Codex],
        node: vec!["first".into()],
        conversation: vec!["conversation".into()],
        role: vec![crate::role::Role::User],
        project: vec!["/work/mash".into()],
        repository: vec!["repo://mash".into()],
        after: Some(2),
        before: Some(3),
        ..Default::default()
    }
    .resolve("first")
    .unwrap();
    let result = search(
        service,
        token,
        &query("voyage-code-4", scoped.filter.clone(), false),
    )
    .await;
    assert_eq!(
        result
            .lexical
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["recent"]
    );
    assert!(
        search(service, token, &query("voyage-code-4", scoped.filter, true))
            .await
            .lexical
            .is_empty()
    );
    let result = search(
        service,
        token,
        &query(
            "voyage-code-4",
            Filter::Provider(vec![Provider::Codex]),
            true,
        ),
    )
    .await;
    assert_eq!(
        result
            .lexical
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["recent", "old", "undated"]
    );
    assert!(
        result
            .lexical
            .iter()
            .all(|h| h.node_id.as_deref() == Some("second"))
    );
    let result = search(
        service,
        OTHER,
        &query(
            "voyage-code-4",
            Filter::Provider(vec![Provider::Codex]),
            false,
        ),
    )
    .await;
    assert_eq!(result.lexical.len(), 3, "groups must remain isolated");
}

async fn verify_core(service: &Service) {
    use crate::{core, storage::Storage};
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("local.sqlite3");
    let mut writer = Storage::open_writer(&database).unwrap();
    let messages = records()
        .into_iter()
        .filter(|m| ["old", "recent", "undated"].contains(&m.id.as_str()))
        .map(|m| crate::silver::NormalizedMessage {
            id: m.id,
            ordinal: m.ordinal,
            context: m.context,
            role: m.role,
            content: m.content,
            dialogue_text: Some(m.projection),
            created_at: m.created_at,
        })
        .collect::<Vec<_>>();
    writer
        .replace_conversation(&crate::silver::Conversation {
            id: "conversation".into(),
            provider: Provider::Codex,
            source: crate::silver::SourceIdentity::file("missing.jsonl"),
            claude_lineage: None,
            title: Some("title-only-marker".into()),
            project: Some("/work/mash".into()),
            repository: Some("repo://mash".into()),
            created_at: Some(1000),
            updated_at: Some(2000),
            messages: messages.into(),
        })
        .unwrap();
    writer.begin_silver_scan(Provider::Codex).unwrap();
    writer.finish_silver_scan(Provider::Codex, true).unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.rebuild_derived_search_state().unwrap();
    writer
        .publish_gold_manifest(&crate::gold::manifest::Manifest::new(
            crate::gold::config::Build::default(),
            "unavailable".into(),
            writer.silver_certificate().unwrap(),
        ))
        .unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let before = std::fs::read(&database).unwrap();
    let config = directory.path().join("node.toml");
    std::fs::write(&config, format!("version=1\nproviders={{}}\n[index]\ndestination='both'\n[shared]\ngroup_id={:?}\nnode_id='first'\napi_url={:?}\ntoken_env='HOME'\n", service.group, service.url)).unwrap();
    for unit in [SearchUnit::Messages, SearchUnit::Threads] {
        let response = core::execute(core::Request {
            paths: core::Paths {
                config: Some(config.clone()),
                db: Some(database.clone()),
                models_dir: Some(directory.path().join("missing-models")),
            },
            operation: core::Operation::Search {
                query: "voyage-code-4".into(),
                intent: Some(Intent::ExactMention),
                limit: Some(10),
                unit: Some(unit),
                filters: Box::new(Selection {
                    provider: vec![Provider::Codex],
                    conversation: vec!["conversation".into()],
                    ..Default::default()
                }),
                shared: false,
                local: false,
            },
        })
        .await
        .unwrap();
        let core::Response::Search(response) = response else {
            panic!("search response")
        };
        assert!(response.shared && response.shared_error.is_none());
        match response.results {
            crate::search_results::Results::Messages(hits) => {
                assert_eq!(
                    hits.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
                    ["recent", "old", "undated"]
                );
                assert!(hits.iter().all(|h| h.node_id.is_none()
                    && h.copies.len() == 1
                    && h.copies[0].node_id.as_deref() == Some("second")));
            }
            crate::search_results::Results::Threads(threads) => {
                assert_eq!(threads.len(), 1);
                assert_eq!(threads[0].matches.len(), 3);
                assert!(threads[0].matches.iter().all(
                    |m| m.copies.len() == 1 && m.copies[0].node_id.as_deref() == Some("second")
                ));
            }
        }
    }
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert!(!directory.path().join("missing-models").exists());
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_exact_broad_queries_fail_explicitly_and_filters_restore_complete_results() {
    let service = Service::qdrant(384).await;
    let setup = events(1);
    let mut upload = vec![setup[0].clone(), setup[1].clone()];
    let rows = (0..=10_000)
        .map(|i| {
            row(
                &format!("budget-{i}"),
                i,
                if i == 10_000 {
                    "use voyage-code-4"
                } else {
                    "voyage code 4"
                },
                Some(i),
            )
        })
        .collect::<Vec<_>>();
    for batch in rows.chunks(128) {
        upload.push(
            serde_json::to_value(Event::Batch(Batch::Messages {
                records: batch.to_vec(),
            }))
            .unwrap(),
        );
    }
    upload.push(serde_json::to_value(Event::Finish).unwrap());
    let started = std::time::Instant::now();
    let response = service.upload(FIRST, &upload).await;
    let status = response.status();
    assert_eq!(status, StatusCode::OK, "{}", response.text().await.unwrap());
    let publication_ms = started.elapsed().as_millis();
    let broad = service
        .request("search", FIRST)
        .json(&query("voyage-code-4", Filter::All(vec![]), false))
        .send()
        .await
        .unwrap();
    assert_eq!(broad.status(), StatusCode::BAD_REQUEST);
    assert!(
        broad.json::<Value>().await.unwrap()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("budget")
    );
    let narrow = search(
        &service,
        FIRST,
        &query("voyage-code-4", Filter::After(10), false),
    )
    .await;
    assert_eq!(
        narrow
            .lexical
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["budget-10000"]
    );
    eprintln!(
        "Qdrant lexical-only 10001-message fixture: publication_ms={publication_ms}; broad scan rejected, time-filtered exact match recovered"
    );
    service.cleanup().await;
}
