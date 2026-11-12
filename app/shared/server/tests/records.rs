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
        INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('message','chat',0,'user','yes'),('confirmation','chat',1,'user','still true');").unwrap();
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
    let receipt = storage.mutate_record(&request).unwrap();
    let confirm = serde_json::from_value(json!({"operation_id":"confirm", "change":{"action":"confirm", "expected":{"id":receipt.record_id,"revision":receipt.revision},"evidence":[{"message_id":"confirmation","quote":"still true","purpose":"report"}]}})).unwrap();
    storage.mutate_record(&confirm).unwrap();
    receipt.record_id
}

#[tokio::test]
async fn native_records_shared_history_cursor_returns_older_revision() {
    let service = Service::canonical().await;
    let id = seed(&service, FIRST);
    let identity = &service.identities[&blake3::hash(FIRST.as_bytes())];
    let path = service
        .silver_dir
        .path()
        .join("silver")
        .join(&identity.group_id)
        .join(format!("{}.sqlite3", identity.node_id));
    let mut storage = crate::storage::Storage::open(&path).unwrap();
    let body = storage.record(&id, false).unwrap().revisions[0]
        .body
        .clone();
    let mutation = serde_json::from_value(json!({"operation_id":"correct", "change":{"action":"revise","expected":{"id":id,"revision":1},"body":body,"evidence":[{"message_id":"message","quote":"yes","purpose":"report"}]}})).unwrap();
    storage.mutate_record(&mutation).unwrap();
    drop(storage);
    let first: Value = service
        .request("record", FIRST)
        .json(&json!({"id":id,"node_id":"first","history":true,"limit":1}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first["revisions"][0]["revision"], 2);
    assert_eq!(first["next_before_revision"], 2);
    let next: Value = service.request("record", FIRST).json(&json!({"id":id,"node_id":"first","history":true,"limit":1,"before_revision":first["next_before_revision"]})).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    assert_eq!(next["revisions"][0]["revision"], 1);
    assert_eq!(
        next["revisions"][0]["confirmation_history"][0]["message_id"],
        "confirmation"
    );
    assert!(next["next_before_revision"].is_null());
    assert_eq!(next["node_id"], "first");
}

#[tokio::test]
async fn native_records_shared_http_pages_preserve_owners_and_group_boundaries_without_views() {
    let service = Service::canonical().await;
    let first = seed(&service, FIRST);
    let second = seed(&service, SECOND);
    assert_ne!(first, second);
    let page: Value = service
        .request("records", FIRST)
        .json(&json!({"shared":true,"include_inactive":true,"kind":"fact","state":"current","review":"established","chat":"chat","query":"claim"}))
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
        .json(&json!({"shared":true,"include_inactive":true,"kind":"fact","state":"current","review":"established","chat":"chat","query":"claim","cursor":page["next_cursor"]}))
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
    assert_eq!(record["revisions"][0]["confirmations"], 1);
    assert_eq!(
        record["revisions"][0]["confirmation_history"][0]["message_id"],
        "confirmation"
    );
    assert!(
        record["revisions"][0]["confirmation_history"][0]["confirmed_at"]
            .as_i64()
            .unwrap()
            > 0
    );
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

#[tokio::test]
async fn native_records_shared_http_applies_dashboard_filters() {
    let service = Service::canonical().await;
    seed(&service, FIRST);
    for (selector, value) in [
        ("kind", "decision"),
        ("state", "retracted"),
        ("review", "candidate"),
        ("chat", "other-chat"),
    ] {
        let mut filters = json!({"shared":true,"include_inactive":true,"node_id":"first"});
        filters[selector] = json!(value);
        let excluded: Value = service
            .request("records", FIRST)
            .json(&filters)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(excluded["records"], json!([]), "{selector} filter");
    }
}
