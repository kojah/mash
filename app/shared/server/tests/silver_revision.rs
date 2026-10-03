//! Canonical revision changes independently of transport, source scans and vectors.
use super::*;
use crate::shared::protocol::NodeState;

async fn state(service: &Service) -> NodeState {
    serde_json::from_value(service.get("state", FIRST).await).unwrap()
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_silver_revision_tracks_canonical_changes_and_atomic_publication() {
    let service = Service::qdrant(384).await;
    assert!(state(&service).await.silver_revision.is_none());
    let initial = snapshot(1);
    service
        .upload(FIRST, &initial)
        .await
        .error_for_status()
        .unwrap();
    let first = state(&service).await.silver_revision.unwrap();
    assert_eq!(first.revision, 1);
    assert_eq!(
        service.get("freshness", FIRST).await["nodes"][0]["silver_revision"],
        serde_json::to_value(&first).unwrap()
    );

    let mut gold = snapshot(2);
    let vector = gold[2]["records"][0]["vector"].as_array_mut().unwrap();
    vector[0] = json!(0);
    vector[1] = json!(127);
    service
        .upload(FIRST, &gold)
        .await
        .error_for_status()
        .unwrap();
    let second = state(&service).await;
    assert_eq!(second.revision, 2);
    assert_eq!(second.silver_revision.as_ref(), Some(&first));

    let checkpoint = vec![
        json!({"type":"start","instance":"snapshot-instance","revision":3,"base_revision":2,"snapshot":false,"forgets":[]}),
        json!({"type":"source","provider":"codex","path_hash":"source","size_bytes":42,"modified_ns":123,"conversation_id":"conversation","interpretation_version":1}),
        json!({"type":"reconcile","provider":"codex","cutoff":0}),
        json!({"type":"scan","provider":"codex","cutoff":0,"interpretation_version":1,"authoritative":true}),
        json!({"type":"finish"}),
    ];
    // Mark the source observed so reconciliation does not remove its thread.
    let mut checkpoint = checkpoint;
    checkpoint.insert(
        2,
        json!({"type":"seen","provider":"codex","hashes":["source"]}),
    );
    service
        .upload(FIRST, &checkpoint)
        .await
        .error_for_status()
        .unwrap();
    assert_eq!(state(&service).await.silver_revision.as_ref(), Some(&first));

    let mut edited = snapshot(4);
    edited[1]["records"][0]["title"] = json!("Edited canonical title");
    assert_eq!(
        service.upload(FIRST, &edited[..3]).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(state(&service).await.silver_revision.as_ref(), Some(&first));
    service
        .upload(FIRST, &edited)
        .await
        .error_for_status()
        .unwrap();
    let changed = state(&service).await.silver_revision.unwrap();
    assert_eq!(changed.revision, 2);
    assert_ne!(changed.fingerprint, first.fingerprint);
    service
        .upload(FIRST, &edited)
        .await
        .error_for_status()
        .unwrap();
    assert_eq!(
        state(&service).await.silver_revision.as_ref(),
        Some(&changed)
    );
    assert_eq!(
        service.upload(FIRST, &initial).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        state(&service).await.silver_revision.as_ref(),
        Some(&changed)
    );

    service
        .request("forget", FIRST)
        .json(&json!({"instance":"snapshot-instance","revision":5,"ids":["conversation"]}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let forgotten = state(&service).await.silver_revision.unwrap();
    assert_eq!(forgotten.revision, 3);
    assert_ne!(forgotten.fingerprint, changed.fingerprint);
    service.cleanup().await;
}
