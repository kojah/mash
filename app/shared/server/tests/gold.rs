use super::*;

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_http_vector_overrides_do_not_change_data_or_group_defaults() {
    let service = Service::qdrant(384).await;
    service
        .upload(FIRST, &snapshot(1))
        .await
        .error_for_status()
        .unwrap();
    let before = service.get("freshness", FIRST).await;
    let canonical = serde_json::to_value(service.state().await).unwrap();
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    for policy in [
        None,
        Some(json!({"algorithm":"hnsw","ef_runtime":80})),
        Some(json!({"algorithm":"exact"})),
        None,
    ] {
        let mut body = json!({"query":"hello","vector":vector});
        if let Some(policy) = policy {
            body["vector_search"] = policy;
        }
        let result: protocol::Candidates = service
            .request("search", FIRST)
            .json(&body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!result.semantic.is_empty());
        assert_eq!(service.get("freshness", FIRST).await, before);
        assert_eq!(
            serde_json::to_value(service.state().await).unwrap(),
            canonical
        );
    }
    for (policy, status) in [
        (
            json!({"algorithm":"hnsw","ef_runtime":0}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"algorithm":"hnsw","ef_runtime":4097}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"algorithm":"exact","ef_runtime":80}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"algorithm":"unknown"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let response = service
            .request("search", FIRST)
            .json(&json!({"query":"hello","vector":vector,"vector_search":policy}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    assert_eq!(
        service
            .request("search", "invalid")
            .json(&json!({"query":"hello","vector":vector,"vector_search":{"algorithm":"hnsw"}}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(service.request("search", FIRST).json(&json!({"query":"hello","intent":"exact-mention","vector_search":{"algorithm":"exact"}})).send().await.unwrap().status(), StatusCode::BAD_REQUEST);
    assert_eq!(service.get("freshness", FIRST).await, before);
    assert_eq!(
        serde_json::to_value(service.state().await).unwrap(),
        canonical
    );
    service.cleanup().await;
}
