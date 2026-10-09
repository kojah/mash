use super::*;
use crate::connectors::{Document, Receipt};

fn batch(name: &str, revision: u64, content: &str) -> Value {
    json!({
        "schema_version":1, "source_id":"github:installation", "batch_id":name,
        "expected_revision":revision,
        "changes":[
            {"op":"upsert-chat","id":"pr:42","title":"Review","metadata":{"repository":"owner/repo"}},
            {"op":"upsert-message","id":"review:1","chat_id":"pr:42","content":"Review comment","author":{"id":"human","kind":"human"},"reply_to":"outside-window"},
            {"op":"upsert-document","id":"note","content":content}
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

fn document(service: &Service, token: &str) -> Document {
    let identity = &service.identities[&blake3::hash(token.as_bytes())];
    let path = service
        .silver_dir
        .path()
        .join("silver")
        .join(&identity.group_id)
        .join(format!("{}.sqlite3", identity.node_id));
    crate::connectors::silver::Store::open(&path)
        .unwrap()
        .document(
            &"github:installation".to_owned().try_into().unwrap(),
            &"note".to_owned().try_into().unwrap(),
        )
        .unwrap()
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
    assert_eq!(document(&service, FIRST).content, "Edited");
    assert!(
        service
            .store
            .require_connector_gold(&service.first, false)
            .await
            .is_err()
    );
    // Excluding this node excludes its unprojected canonical input too.
    service
        .store
        .require_connector_gold(&service.first, true)
        .await
        .unwrap();
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
        assert_eq!(document(&service, token).content, content);
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
    assert_eq!(document(&service, FIRST).content, "first node");
    assert_eq!(document(&service, SECOND).content, "second node");
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
