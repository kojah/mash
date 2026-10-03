//! Weighted admission preserves active revisions and leaves reads independent.
use super::*;
use crate::shared::snapshot_budget::Budget;

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_snapshot_budget_rejects_overweight_and_busy_mutations_without_revision_changes() {
    contract().await;
}

async fn assert_error(response: reqwest::Response, status: StatusCode, retryable: bool) {
    assert_eq!(response.status(), status);
    let error: AppError = response.json().await.unwrap();
    assert_eq!(error.error.retryable, retryable);
    assert_eq!(
        error.error.kind,
        if retryable {
            crate::ErrorKind::SharedIndex
        } else {
            crate::ErrorKind::Usage
        }
    );
    assert!(error.error.message.contains(if retryable {
        "snapshot capacity"
    } else {
        "max_mutation_snapshot_mib"
    }));
}

fn large_snapshot(revision: i64) -> Vec<Value> {
    let mut large = snapshot(revision);
    let mut batch: protocol::Event = serde_json::from_value(large[2].clone()).unwrap();
    if let protocol::Event::Batch(protocol::Batch::Messages { records }) = &mut batch {
        records[2].content = "canonical context ".repeat(50000);
    }
    large[2] = serde_json::to_value(batch).unwrap();
    large
}

async fn contract() {
    let budget = Budget::new(1).unwrap();
    let service = Service::with_budget(true, 384, 64 * 1024 * 1024, FIRST, budget.clone()).await;
    let large = large_snapshot(1);
    assert_eq!(service.upload(FIRST, &large).await.status(), StatusCode::OK);
    let before = service.get("freshness", FIRST).await;
    let replacement = large_snapshot(2);
    assert_error(
        service.upload(FIRST, &replacement).await,
        StatusCode::BAD_REQUEST,
        false,
    )
    .await;
    assert_eq!(service.get("freshness", FIRST).await, before);

    // Weighted pressure comes from another job; the count-based upload slots are free.
    let held = budget.reserve(0, 1024 * 1024).unwrap();
    assert_error(
        service.upload(FIRST, &snapshot(2)).await,
        StatusCode::SERVICE_UNAVAILABLE,
        true,
    )
    .await;
    assert_error(
        service.upload(SECOND, &snapshot(1)).await,
        StatusCode::SERVICE_UNAVAILABLE,
        true,
    )
    .await;
    let forget = protocol::Forget {
        instance: "snapshot-instance".into(),
        revision: Some(3),
        ids: vec!["conversation".into()],
    };
    assert_error(
        service
            .request("forget", FIRST)
            .json(&forget)
            .send()
            .await
            .unwrap(),
        StatusCode::SERVICE_UNAVAILABLE,
        true,
    )
    .await;
    assert_eq!(service.get("freshness", FIRST).await, before);
    let messages: Vec<crate::storage::Message> = service
        .request("view", FIRST)
        .json(&protocol::View {
            node_id: "first".into(),
            id: "context".into(),
            context: 0,
            content: crate::storage::MessageContent::Canonical,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "canonical context ".repeat(50000));
    drop(held);

    assert_eq!(
        service.upload(FIRST, &snapshot(2)).await.status(),
        StatusCode::OK
    );
    // A planning failure also restores its weighted reservation.
    let response = service
        .request("index", FIRST)
        .header("content-type", "application/x-ndjson")
        .body("{\"type\":\"finish\"}\n")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        service
            .request("forget", FIRST)
            .json(&forget)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let counts: crate::storage::Counts =
        serde_json::from_value(service.get("status", FIRST).await).unwrap();
    assert_eq!(counts.messages, 0);
    service.cleanup().await;
}
