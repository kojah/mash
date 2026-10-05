use super::*;

#[tokio::test]
async fn context_batches_authenticate_and_validate_before_backend_reads() {
    let service = Service::new(false).await;
    let anchor =
        json!({"node_id":"first","id":"message","context":2,"content":"search-projection"});
    assert_eq!(
        service
            .request("contexts", "wrong-token")
            .json(&json!({"anchors":[anchor.clone()]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for input in [
        json!({"anchors":[]}),
        json!({"anchors":vec![anchor.clone(); 65]}),
        json!({"anchors":[anchor.clone(),anchor.clone()]}),
        json!({"anchors":[{"node_id":"first","id":"message","context":3,"content":"search-projection"}]}),
        json!({"anchors":[{"node_id":"first","id":"message","context":2}]}),
        json!({"anchors":[{"node_id":"first","id":"","context":2,"content":"search-projection"}]}),
    ] {
        assert_eq!(
            service
                .request("contexts", FIRST)
                .json(&input)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        service
            .request("contexts", FIRST)
            .json(&json!({"anchors":[anchor]}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    service.task.abort();
}
