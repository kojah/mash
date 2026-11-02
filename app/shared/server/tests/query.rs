//! Typed shared-silver queries: group scope, node selection and no SQL field.

use super::*;

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn shared_typed_queries_are_group_scoped_and_reject_sql() {
    let service = Service::qdrant(384).await;
    assert_eq!(
        service.upload(FIRST, &snapshot(1)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        service.upload(SECOND, &snapshot(1)).await.status(),
        StatusCode::OK
    );

    let response = service
        .request("query", FIRST)
        .json(&json!({"request": {"target": "messages", "group_by": "source"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["columns"], json!(["group_key", "count"]));
    assert_eq!(body["rows"], json!([{"group_key": "codex", "count": 3}]));
    assert_eq!(body["truncated"], false);

    // An in-group node can be named explicitly.
    let response = service
        .request("query", FIRST)
        .json(&json!({"node": "second", "request": {"target": "conversations"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // A node in another group is invisible even when the name matches.
    let response = service
        .request("query", OTHER)
        .json(&json!({"node": "second", "request": {"target": "messages"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // SQL is not part of the shared contract.
    let response = service
        .request("query", FIRST)
        .json(&json!({"request": {"target": "messages", "sql": "SELECT 1"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    service.cleanup().await;
}
