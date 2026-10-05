use super::*;

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with disposable Qdrant 1.19+"]
async fn qdrant_http_masks_legacy_credentials_and_accepts_redacted_uploads() {
    const APP_TOKEN: &str = "xapp-1-A1B2C3D4E5-1234567890123-abcdef0123456789abcdef0123456789";
    let service = Service::qdrant(384).await;
    let mut events = snapshot(1);
    events[1]["records"][0]["title"] = json!("password=\"rosebud\"");
    let content =
        format!("credentialneedle password=\"hunter2\" token = fetch_token(); {APP_TOKEN}");
    events[2]["records"][0]["content"] = json!(content);
    events[2]["records"][0]["projection"] = json!(content);
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
        (
            "contexts",
            json!({"anchors":[{"node_id":"first","id":"searchable","context":0,"content":"search-projection"}]}),
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
            !body.contains("hunter2") && !body.contains("rosebud") && !body.contains(APP_TOKEN),
            "{body}"
        );
        assert!(
            body.contains("REDACTED_SECRET") || body.contains("*****"),
            "{body}"
        );
        if matches!(route, "view" | "contexts") {
            assert!(body.contains("token = fetch_token()"), "{body}");
        }
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
    assert!(!body.contains("hunter2") && !body.contains("rosebud") && !body.contains(APP_TOKEN));
    assert!(body.contains("token = fetch_token()"));
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
