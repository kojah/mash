use super::*;
use crate::connectors::{Object, Receipt};
use crate::storage::SearchHit;

fn batch(name: &str, revision: u64, content: &str) -> Value {
    json!({
        "schema_version":2, "source_id":"github:installation", "batch_id":name,
        "expected_revision":revision,
        "changes":[
            {"op":"upsert-object","id":"pr:42","role":"container","kind":"github:pull-request","title":"Review","metadata":{"repository":"owner/repo"}},
            {"op":"upsert-object","id":"review:1","role":"entry","kind":"github:review-comment","parent":"pr:42","content":"Review comment","author":{"id":"human","kind":"human"},"reply_to":"outside-window"},
            {"op":"upsert-object","id":"note","role":"document","kind":"obsidian:note","content":content}
        ]
    })
}

async fn upload(service: &Service, token: &str, batch: &Value) -> reqwest::Response {
    service
        .request("connector-batches", token)
        .json(batch)
        .send()
        .await
        .unwrap()
}

fn document(service: &Service, token: &str) -> String {
    let identity = &service.identities[&blake3::hash(token.as_bytes())];
    let path = service
        .silver_dir
        .path()
        .join("silver")
        .join(&identity.group_id)
        .join(format!("{}.sqlite3", identity.node_id));
    crate::connectors::silver::Store::open(&path)
        .unwrap()
        .object(
            &"github:installation".to_owned().try_into().unwrap(),
            &"note".to_owned().try_into().unwrap(),
        )
        .unwrap()
        .and_then(|object: Object| object.content)
        .unwrap()
}

#[tokio::test]
async fn connector_http_receipts_replay_after_later_batches_and_conflicts_do_not_mutate() {
    let service = Service::canonical().await;
    let first = batch("first", 0, "Original");
    let receipt: Receipt = upload(&service, FIRST, &first)
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(receipt.source_revision, 1);
    assert!(!receipt.gold_ready);
    let next: Receipt = upload(&service, FIRST, &batch("second", 1, "Edited"))
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(next.source_revision, 2);
    assert!(next.silver_revision > receipt.silver_revision);
    let replay: Receipt = upload(&service, FIRST, &first)
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(replay, receipt);
    for request in [
        batch("first", 0, "PRIVATE-CANARY"),
        batch("stale", 0, "PRIVATE-CANARY"),
    ] {
        let conflict = upload(&service, FIRST, &request).await;
        assert_eq!(conflict.status(), StatusCode::CONFLICT);
        let error = conflict.text().await.unwrap();
        assert!(!error.contains("PRIVATE-CANARY"));
    }
    // Without Qdrant, gold synchronization fails after the silver commit; the
    // durable receipt is still returned and silver holds the latest batch.
    assert_eq!(document(&service, FIRST), "Edited");
}

#[tokio::test]
async fn connector_http_identity_is_authorized_by_token_not_payload_or_headers() {
    let service = Service::canonical().await;
    for (token, content) in [
        (FIRST, "first node"),
        (SECOND, "second node"),
        (OTHER, "other group"),
    ] {
        upload(&service, token, &batch("same-id", 0, content))
            .await
            .error_for_status()
            .unwrap();
        assert_eq!(document(&service, token), content);
    }
    let unauthorized = upload(&service, "invalid", &batch("third", 1, "private")).await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let impersonation = service
        .request("connector-batches", FIRST)
        .header("x-mash-node", "second")
        .json(&batch("third", 1, "private"))
        .send()
        .await
        .unwrap();
    assert_eq!(impersonation.status(), StatusCode::FORBIDDEN);
    let mut forged = batch("third", 1, "private");
    forged["node_id"] = json!("second");
    assert_eq!(
        upload(&service, FIRST, &forged).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(document(&service, FIRST), "first node");
    assert_eq!(document(&service, SECOND), "second node");
}

#[tokio::test]
async fn connector_http_limits_media_and_mutation_capacity_fail_before_commit() {
    let service = Service::with_backend(TestBackend::Silver, 384, 1024, FIRST).await;
    let oversized = batch("first", 0, &"x".repeat(2048));
    assert_eq!(
        upload(&service, FIRST, &oversized).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let malformed = service
        .request("connector-batches", FIRST)
        .header("content-type", "application/json")
        .body("{PRIVATE-CANARY")
        .send()
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert!(!malformed.text().await.unwrap().contains("PRIVATE-CANARY"));
    let wrong_media = service
        .request("connector-batches", FIRST)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_media.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let capacity = service.uploads.acquire_many(4).await.unwrap();
    assert_eq!(
        upload(&service, FIRST, &batch("first", 0, "ok"))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(capacity);
    let receipt: Receipt = upload(&service, FIRST, &batch("first", 0, "ok"))
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(receipt.source_revision, 1);
}

fn exact(text: &str) -> crate::shared::protocol::Search {
    crate::shared::protocol::Search {
        match_mode: crate::gold::matching::Mode::Strict,
        intent: crate::gold::exact::Intent::ExactMention,
        vector_policy: None,
        unit: crate::search_results::SearchUnit::Messages,
        query: text.into(),
        generation: String::new(),
        vector: vec![],
        provider: None,
        days: None,
        exclude_self: false,
        filter: None,
    }
}

async fn found(service: &Service, input: &crate::shared::protocol::Search) -> Vec<SearchHit> {
    let response = service
        .request("search", FIRST)
        .json(input)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let candidates: crate::shared::protocol::Candidates = serde_json::from_str(&body).unwrap();
    candidates
        .lexical
        .into_iter()
        .chain(candidates.semantic)
        .collect()
}

const MESSAGE: &str = "connector#entry#github:installation#review:1";
const NOTE: &str = "connector#document#github:installation#note";

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL and installed MASH_TEST_MODELS_DIR"]
async fn qdrant_connector_records_are_searchable_and_follow_shared_silver() {
    let models = std::path::PathBuf::from(std::env::var("MASH_TEST_MODELS_DIR").unwrap());
    let service = Service::new(true).await;
    // A connector-only node: it never uploads native history.
    upload(&service, FIRST, &batch("first", 0, "Zeppelin runbook"))
        .await
        .error_for_status()
        .unwrap();
    let hits = found(&service, &exact("zeppelin")).await;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, NOTE);
    assert_eq!(hits[0].object.role, crate::connectors::Role::Document);
    assert_eq!(
        hits[0].source,
        crate::source::Source::Connector("github:installation".into())
    );
    assert_eq!(hits[0].node_id.as_deref(), Some("first"));
    let mut vector = crate::gold::shared::embed_texts(
        &models,
        None,
        &mut None,
        &["Review\nReview comment"],
        &mut crate::gold::shared::Build::default(),
    )
    .await
    .unwrap();
    let related = crate::shared::protocol::Search {
        intent: crate::gold::exact::Intent::RelatedDiscussion,
        query: "review".into(),
        generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
        vector: vector.pop().unwrap(),
        ..exact("review")
    };
    let semantic = found(&service, &related).await;
    let message = semantic.iter().find(|hit| hit.id == MESSAGE).unwrap();
    assert_eq!(
        message.author_kind,
        Some(crate::connectors::AuthorKind::Human)
    );
    assert!(
        semantic
            .iter()
            .any(|hit| hit.semantic_score.is_some() && hit.id == MESSAGE)
    );
    // Edits replace points; a provider filter never matches connector records.
    upload(&service, FIRST, &batch("second", 1, "Airship runbook"))
        .await
        .error_for_status()
        .unwrap();
    assert!(found(&service, &exact("zeppelin")).await.is_empty());
    assert_eq!(found(&service, &exact("airship")).await[0].id, NOTE);
    let native_only = crate::shared::protocol::Search {
        filter: Some(crate::search_filter::Filter::Provider(vec![
            crate::provider::Provider::Codex,
        ])),
        ..exact("airship")
    };
    assert!(found(&service, &native_only).await.is_empty());
    verify_view_and_listing(&service).await;
    // A native upload recertifies the node without losing connector points.
    let response = service.upload(FIRST, &snapshot(1)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().await.unwrap()
    );
    assert_eq!(found(&service, &exact("airship")).await[0].id, NOTE);
    // Kinds select connector payloads and native points alike.
    let kind = |kinds: &[&str], text: &str| crate::shared::protocol::Search {
        filter: Some(crate::search_filter::Filter::Kind(
            kinds.iter().map(|kind| (*kind).to_owned()).collect(),
        )),
        ..exact(text)
    };
    assert_eq!(
        found(&service, &kind(&["obsidian:note"], "airship")).await[0].id,
        NOTE
    );
    assert!(
        found(&service, &kind(&["github:review-comment"], "airship"))
            .await
            .is_empty()
    );
    assert_eq!(
        found(&service, &kind(&["codex:message"], "hello")).await[0].id,
        "searchable"
    );
    assert!(
        found(&service, &kind(&["obsidian:note"], "hello"))
            .await
            .is_empty()
    );
    // Replay rebuilds every point, connector points included, from silver.
    service
        .store
        .replay(&service.first, &models, None)
        .await
        .unwrap();
    assert_eq!(found(&service, &exact("airship")).await[0].id, NOTE);
    let gold = service
        .store
        .gold_state(&service.first)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(gold.connectors.len(), 2);
    assert!(gold.certified);
    service.cleanup().await;
}

async fn verify_view_and_listing(service: &Service) {
    let view: Vec<crate::storage::Message> = service
        .request("view", FIRST)
        .json(&serde_json::json!({"node_id": "first", "id": MESSAGE, "context": 1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(view[0].content, "Review comment");
    let chats: Value = service
        .request("chats", FIRST)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        chats["chats"]
            .as_array()
            .unwrap()
            .iter()
            .any(|chat| chat["chat_id"] == "connector#container#github:installation#pr:42")
    );
}
