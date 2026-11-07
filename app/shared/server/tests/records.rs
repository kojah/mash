use super::*;

fn seed(service: &Service, token: &str) -> String {
    let identity = &service.identities[&blake3::hash(token.as_bytes())];
    let path = service
        .silver_dir
        .path()
        .join("silver")
        .join(&identity.group_id)
        .join(format!("{}.sqlite3", identity.node_id));
    let mut storage = crate::storage::Storage::open(&path).unwrap();
    let connection = crate::storage::test_support::connect(&path);
    connection.execute_batch("INSERT INTO chats(id,provider) VALUES('chat','codex');
        INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('message','chat',0,'user','yes');").unwrap();
    let head =
        serde_json::from_value(json!({"instance":"test", "revision":1, "version":"one"})).unwrap();
    crate::shared::silver_store::put_head(&connection, &head).unwrap();
    drop(connection);
    let request = serde_json::from_value(json!({
        "operation_id":"claim", "change": {"action":"create", "scope":{"kind":"personal"},
        "body":{"statement":"same claim", "recorded_by":"test", "review":"established",
        "details":{"kind":"fact", "state":"current", "reported_by":"user"}},
        "evidence":[{"message_id":"message", "quote":"yes", "purpose":"report"}]}
    }))
    .unwrap();
    storage.mutate_record(&request).unwrap().record_id
}

#[tokio::test]
async fn native_records_shared_http_pages_preserve_owners_and_group_boundaries_without_views() {
    let service = Service::canonical().await;
    let first = seed(&service, FIRST);
    let second = seed(&service, SECOND);
    assert_ne!(first, second);
    let page: Value = service
        .request("records", FIRST)
        .json(&json!({"shared":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["records"][0]["id"], first);
    assert_eq!(page["records"][0]["node_id"], "first");
    let next: Value = service
        .request("records", FIRST)
        .json(&json!({"shared":true,"cursor":page["next_cursor"]}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(next["records"][0]["id"], second);
    assert_eq!(next["records"][0]["node_id"], "second");
    assert!(next.get("next_cursor").is_none());
    let record: Value = service
        .request("record", FIRST)
        .json(&json!({"id":second,"node_id":"second","history":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(record["id"], second);
    assert_ne!(page["records"][0]["owner"], record["owner"]);
    assert_eq!(record["revisions"][0]["confirmations"], 0);
    let other: Value = service
        .request("records", OTHER)
        .json(&json!({"shared":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(other["records"], json!([]));
    assert!(
        !service
            .request("record", OTHER)
            .json(&json!({"id":second,"node_id":"second"}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(
        service
            .request("records", "invalid-token")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        !service
            .request("records", FIRST)
            .json(&json!({"cursor":{"node_id":"unknown", "offset":0}}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
}
