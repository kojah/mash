use super::*;
use crate::shared::{gold_rebuild::Event, protocol::NodeState};
use crate::silver::certificate::{Certificate, Completeness, INTERPRETATION_VERSION};

async fn state(service: &Service) -> NodeState {
    serde_json::from_value(service.get("state", FIRST).await).unwrap()
}

async fn upload(service: &Service, events: &[Value]) -> reqwest::Response {
    use std::fmt::Write;
    let mut body = String::new();
    for event in events {
        writeln!(&mut body, "{event}").unwrap();
    }
    service
        .request("gold", FIRST)
        .header("content-type", "application/x-ndjson")
        .body(body)
        .send()
        .await
        .unwrap()
}

fn replacement(state: &NodeState) -> Vec<Value> {
    let mut vector = vec![0.0; 384];
    vector[0] = -127.0;
    let mut opposite = vector.clone();
    opposite[0] = 127.0;
    vec![
        serde_json::to_value(Event::Start {
            build: crate::gold::config::Build::default(),
            publication_revision: state.revision,
            silver: state.silver_revision.clone().unwrap(),
            generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
        })
        .unwrap(),
        json!({"type":"embeddings","records":[
            {"message_id":"searchable","vector":vector},
            {"message_id":"opposite","vector":opposite}]}),
        json!({"type":"finish"}),
    ]
}

async fn best(service: &Service) -> String {
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    let response: Value = service
        .request("search", FIRST)
        .json(&json!({
            "query":"history","vector":vector,"provider":null,"days":null
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    response["semantic"][0]["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_gold_only_rebuild_preserves_silver_and_rejects_invalid_replacements() {
    let service = Service::qdrant(384).await;
    service
        .upload(FIRST, &snapshot(1))
        .await
        .error_for_status()
        .unwrap();
    let uncertified = state(&service).await;
    assert_eq!(
        upload(&service, &replacement(&uncertified)).await.status(),
        StatusCode::CONFLICT
    );
    let mut canonical = snapshot(2);
    canonical[0]["source_silver"] = serde_json::to_value(Certificate {
        revision: 2,
        interpretation_version: INTERPRETATION_VERSION,
        completeness: Completeness::Complete,
        rejected_sources: 0,
    })
    .unwrap();
    service
        .upload(FIRST, &canonical)
        .await
        .error_for_status()
        .unwrap();
    let before = state(&service).await;
    let freshness = service.get("freshness", FIRST).await;
    let canonical_read = retained(&service).await;
    assert_eq!(best(&service).await, "searchable");
    let events = replacement(&before);
    assert_rejected_replacements(&service, &events, &before).await;
    upload(&service, &events).await.error_for_status().unwrap();
    assert_eq!(state(&service).await, before);
    assert_eq!(service.get("freshness", FIRST).await, freshness);
    assert_eq!(best(&service).await, "opposite");
    assert_eq!(retained(&service).await, canonical_read);
    // The next SQLite snapshot revision still publishes its canonical edit.
    canonical[0]["revision"] = json!(3);
    canonical[1]["records"][0]["title"] = json!("new canonical title");
    service
        .upload(FIRST, &canonical)
        .await
        .error_for_status()
        .unwrap();
    assert_eq!(
        upload(&service, &events).await.status(),
        StatusCode::CONFLICT
    );
    assert_ne!(
        state(&service).await.silver_revision,
        before.silver_revision
    );
    service.cleanup().await;
}

async fn assert_rejected_replacements(service: &Service, events: &[Value], before: &NodeState) {
    for defect in 0..15 {
        let mut invalid = events.to_vec();
        match defect {
            0 => {
                invalid.pop();
            }
            1 => {
                invalid[1]["records"].as_array_mut().unwrap().pop();
            }
            2 => invalid[1]["records"][0]["message_id"] = json!("context"),
            3 => invalid[1]["records"][0]["message_id"] = json!("unknown"),
            4 => invalid[1]["records"][1]["message_id"] = json!("searchable"),
            5 => invalid[1]["records"][0]["vector"] = json!([1]),
            6 => {
                invalid.insert(2, snapshot(1)[1].clone());
            }
            7 => invalid[0]["silver"]["revision"] = json!(999),
            8 => invalid[0]["generation"] = json!("wrong-model"),
            9 => invalid[0]["publication_revision"] = json!(999),
            10 => invalid[1]["records"][0]["vector"] = json!(vec![0; 384]),
            11 => invalid.push(json!({"type":"finish"})),
            13 => invalid[0]["build"]["embedding_text"] = json!("title-and-dialogue"),
            14 => invalid[0]["build"]["embedding_text"] = json!("unknown"),
            _ => invalid[1]["records"][0]["content"] = json!("canonical mutation forbidden"),
        }
        assert!(
            !upload(service, &invalid).await.status().is_success(),
            "defect {defect}"
        );
        assert_eq!(state(service).await, *before);
        assert_eq!(best(service).await, "searchable");
    }
}

async fn retained(service: &Service) -> Value {
    let data = service.get("silver", FIRST).await;
    let decoded: crate::shared::silver_read::Retained =
        serde_json::from_value(data.clone()).unwrap();
    assert_eq!(decoded.dimensions, 384);
    assert_eq!(decoded.history.messages.len(), 3);
    assert_eq!(decoded.history.messages["context"].projection, "");
    for message in data["history"]["messages"].as_object().unwrap().values() {
        assert!(message.get("vector").is_none() && message.get("generation").is_none());
    }
    for source in data["history"]["sources"].as_object().unwrap().values() {
        assert!(source.get("ready").is_none());
    }
    assert_eq!(
        decoded.history.messages["searchable"].content,
        "hello shared history"
    );
    data
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_source_facts_remain_unchanged_when_gold_readiness_changes() {
    let service = Service::qdrant(384).await;
    let mut events = snapshot(1);
    events[0]["snapshot"] = json!(false);
    events[0]["base_revision"] = json!(0);
    events.pop();
    events.extend([
        json!({"type":"source","provider":"codex","path_hash":"source",
            "size_bytes":10,"modified_ns":20,"fingerprint":"stamp",
            "interpretation_version":INTERPRETATION_VERSION,
            "conversation_id":"conversation","ready":false}),
        json!({"type":"seen","provider":"codex","hashes":["source"]}),
        json!({"type":"reconcile","provider":"codex","cutoff":null}),
        json!({"type":"scan","provider":"codex","cutoff":null,
            "interpretation_version":INTERPRETATION_VERSION,"authoritative":true}),
        json!({"type":"finish"}),
    ]);
    service
        .upload(FIRST, &events)
        .await
        .error_for_status()
        .unwrap();
    let before = retained(&service).await;
    let sources = &before["history"]["sources"];
    assert_eq!(sources.as_object().unwrap().len(), 1);
    let first = state(&service).await;
    assert!(
        first.sources[0].ready,
        "input readiness must not override gold"
    );

    events[0]["revision"] = json!(2);
    events[0]["base_revision"] = json!(1);
    events[3]["ready"] = json!(true);
    for message in events[2]["records"].as_array_mut().unwrap() {
        message["vector"] = Value::Null;
        message["generation"] = Value::Null;
    }
    service
        .upload(FIRST, &events)
        .await
        .error_for_status()
        .unwrap();
    let incomplete = state(&service).await;
    assert!(!incomplete.sources[0].ready);
    assert_eq!(retained(&service).await["history"], before["history"]);
    assert_eq!(incomplete.silver_revision, first.silver_revision);

    upload(&service, &replacement(&incomplete))
        .await
        .error_for_status()
        .unwrap();
    assert!(state(&service).await.sources[0].ready);
    assert_eq!(retained(&service).await["history"], before["history"]);
    assert_eq!(
        state(&service).await.silver_revision,
        incomplete.silver_revision
    );
    service.cleanup().await;
}
