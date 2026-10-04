use super::*;

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with disposable Qdrant 1.19+"]
async fn qdrant_http_masks_legacy_credentials_and_accepts_redacted_uploads() {
    let service = Service::qdrant(384).await;
    let mut events = snapshot(1);
    events[1]["records"][0]["title"] = json!("password=\"rosebud\"");
    events[2]["records"][0]["content"] = json!("credentialneedle password=\"hunter2\"");
    events[2]["records"][0]["projection"] = json!("credentialneedle password=\"hunter2\"");
    // An old client can still publish raw data; current read boundaries mask it.
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::OK
    );
    for (route, input) in [
        (
            "view",
            json!({"node_id":"first","id":"searchable","context":0}),
        ),
        ("threads", json!({})),
        (
            "search",
            json!({"query":"credentialneedle","intent":"exact-mention","unit":"messages","exclude_self":false}),
        ),
    ] {
        let response = service
            .request(route, FIRST)
            .json(&input)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        let body = response.text().await.unwrap();
        assert!(
            !body.contains("hunter2") && !body.contains("rosebud"),
            "{body}"
        );
        assert!(
            body.contains("REDACTED_SECRET") || body.contains("*****"),
            "{body}"
        );
    }
    events[0]["revision"] = json!(2);
    let mut file = Vec::new();
    for event in events {
        crate::shared::write_event(
            &mut file,
            &serde_json::from_value::<protocol::Event>(event).unwrap(),
        )
        .unwrap();
    }
    let body = String::from_utf8(file).unwrap();
    assert!(!body.contains("hunter2") && !body.contains("rosebud"));
    assert_eq!(
        service
            .request("index", FIRST)
            .header("content-type", "application/x-ndjson")
            .body(body)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    service.cleanup().await;
}
