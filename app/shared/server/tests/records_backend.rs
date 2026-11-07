//! Native records through the real publication protocol and disposable Qdrant.
use super::*;

fn record_batch(connection: &rusqlite::Connection) -> Value {
    let mut records = Vec::new();
    crate::storage::records::transfer::visit_shared(
        connection,
        None,
        crate::redaction::Policy::new(false),
        |row| {
            records.push(row);
            Ok(())
        },
    )
    .unwrap();
    serde_json::to_value(protocol::Event::Batch(protocol::Batch::Records { records })).unwrap()
}

async fn documents(service: &Service) -> Value {
    let response: Value = reqwest::Client::new()
        .post(format!(
            "{}/collections/mash_views_{}_messages/points/scroll",
            std::env::var("MASH_TEST_QDRANT_URL").unwrap(),
            blake3::hash(service.group.as_bytes()).to_hex()
        ))
        .json(&json!({"limit":100,"with_payload":true,"with_vector":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(response["result"]["next_page_offset"].is_null());
    response["result"]["points"].clone()
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn native_records_qdrant_publication_retry_preserves_chat_certificate() {
    let service = Service::qdrant(384).await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.sqlite3");
    let mut storage = crate::storage::Storage::open(&path).unwrap();
    let connection = crate::storage::test_support::connect(&path);
    connection.execute_batch("INSERT INTO chats(id,provider) VALUES('conversation','codex');
        INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('searchable','conversation',0,'user','hello shared history');").unwrap();
    let create = serde_json::from_value(json!({
        "operation_id":"create", "change":{"action":"create","scope":{"kind":"personal"},
        "body":{"statement":"Shared history is available", "review":"established", "recorded_by":"test agent",
        "details":{"kind":"fact","state":"current","reported_by":"user"}},
        "evidence":[{"message_id":"searchable","quote":"hello shared history","purpose":"report"}]}
    })).unwrap();
    let receipt = storage.mutate_record(&create).unwrap();
    let mut initial = snapshot(1);
    initial.insert(initial.len() - 1, record_batch(&connection));
    service
        .upload(FIRST, &initial)
        .await
        .error_for_status()
        .unwrap();
    let certificate = service.state().await.silver_revision.unwrap();
    let before = documents(&service).await;
    assert_eq!(before.as_array().unwrap().len(), 2);

    let mut body = storage.record(&receipt.record_id, false).unwrap().revisions[0]
        .body
        .clone();
    body.statement = "Shared history remains available".into();
    let revise = serde_json::from_value(json!({"operation_id":"revise", "change":{
        "action":"revise","expected":{"id":receipt.record_id,"revision":1},"body":body,
        "evidence":[{"message_id":"searchable","quote":"hello shared history","purpose":"report"}]
    }}))
    .unwrap();
    storage.mutate_record(&revise).unwrap();
    let delta = vec![
        json!({"type":"start","instance":"snapshot-instance","revision":2,"base_revision":1,"snapshot":false,"forgets":[]}),
        record_batch(&connection),
        json!({"type":"finish"}),
    ];
    for _ in 0..2 {
        service
            .upload(FIRST, &delta)
            .await
            .error_for_status()
            .unwrap();
        assert_eq!(
            service.state().await.silver_revision.as_ref(),
            Some(&certificate)
        );
        assert_eq!(documents(&service).await, before);
    }
    let record: Value = service
        .request("record", FIRST)
        .json(&json!({"id":receipt.record_id,"node_id":"first","history":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(record["revisions"].as_array().unwrap().len(), 2);
    assert_eq!(
        record["revisions"][0]["body"]["statement"],
        "Shared history remains available"
    );

    let forget = serde_json::from_value(json!({"operation_id":"forget", "change":{
        "action":"forget", "expected":{"id":receipt.record_id,"revision":2}
    }}))
    .unwrap();
    storage.mutate_record(&forget).unwrap();
    let changed = vec![
        json!({"type":"start","instance":"snapshot-instance","revision":3,"base_revision":2,"snapshot":false,"forgets":[]}),
        record_batch(&connection),
        json!({"type":"finish"}),
    ];
    service
        .upload(FIRST, &changed)
        .await
        .error_for_status()
        .unwrap();
    assert!(
        !service
            .request("record", FIRST)
            .json(&json!({"id":receipt.record_id,"node_id":"first"}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(documents(&service).await, before);
    assert_eq!(
        service.state().await.silver_revision.as_ref(),
        Some(&certificate)
    );
    service.cleanup().await;
}
