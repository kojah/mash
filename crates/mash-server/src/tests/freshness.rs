use super::*;

async fn read(service: &Service, token: &str) -> crate::shared::Freshness {
    serde_json::from_value(service.get("freshness", token).await).unwrap()
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.20+"]
async fn qdrant_replayed_unpublished_node_does_not_block_published_search() {
    let service = Service::qdrant(384).await;
    service
        .upload(FIRST, &snapshot(1))
        .await
        .error_for_status()
        .unwrap();
    let unpublished = service
        .identities
        .get(&blake3::hash(SECOND.as_bytes()))
        .unwrap();
    let models = tempfile::tempdir().unwrap();
    let replayed = service
        .store
        .replay(unpublished, models.path())
        .await
        .unwrap();
    assert_eq!(replayed.messages, 0);
    let empty = service
        .store
        .compatible_views(unpublished)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        read(&service, FIRST).await.unpublished_nodes,
        Some(vec!["second".into()])
    );
    let response = reqwest::Client::new()
        .post(format!("{}/v1/search", service.url))
        .bearer_auth(FIRST)
        .json(&serde_json::json!({"query":"hello", "vector":vec![1.0; 384]}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");
    let candidates: crate::shared::protocol::Candidates = serde_json::from_str(&body).unwrap();
    assert!(!candidates.semantic.is_empty());
    assert!(
        candidates
            .semantic
            .iter()
            .all(|hit| hit.node_id.as_deref() == Some("first"))
    );
    // Once a node publishes, its uncertified views must still fail closed.
    service
        .upload(SECOND, &snapshot(1))
        .await
        .error_for_status()
        .unwrap();
    service.store.put_views(unpublished, &empty).await.unwrap();
    let response = reqwest::Client::new()
        .post(format!("{}/v1/search", service.url))
        .bearer_auth(FIRST)
        .json(&serde_json::json!({"query":"hello", "vector":vec![1.0; 384]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.20+"]
async fn qdrant_freshness_reports_unpublished_members_only_in_the_authorized_group() {
    let service = Service::qdrant(384).await;
    let initial = read(&service, FIRST).await;
    assert!(initial.nodes.is_empty());
    let activity = serde_json::to_value(&initial.activity).unwrap();
    assert!(activity["capacity"].as_u64().is_some());
    assert_eq!(activity["occupied"], 0);
    assert_eq!(activity["active"], serde_json::json!([]));
    assert_eq!(
        initial.unpublished_nodes.as_deref(),
        Some(["first".to_owned(), "second".to_owned()].as_slice())
    );
    assert_eq!(
        read(&service, OTHER).await.unpublished_nodes,
        Some(vec!["first".into()])
    );

    service
        .upload(FIRST, &snapshot(1))
        .await
        .error_for_status()
        .unwrap();
    let published = read(&service, FIRST).await;
    assert_eq!(published.nodes.len(), 1);
    assert_eq!(published.nodes[0].node_id, "first");
    assert!(published.nodes[0].last_synced_at.is_some());
    assert_eq!(published.unpublished_nodes, Some(vec!["second".into()]));
    // The other group has a different node with the same name; publishing one
    // must not certify the other or disclose this group's second member.
    assert_eq!(
        read(&service, OTHER).await.unpublished_nodes,
        Some(vec!["first".into()])
    );

    service
        .upload(SECOND, &snapshot(1))
        .await
        .error_for_status()
        .unwrap();
    assert_eq!(read(&service, FIRST).await.unpublished_nodes, Some(vec![]));
    service
        .upload(OTHER, &snapshot(1))
        .await
        .error_for_status()
        .unwrap();
    assert_eq!(read(&service, OTHER).await.unpublished_nodes, Some(vec![]));
    service.cleanup().await;
}

#[test]
fn absent_membership_scope_is_unknown_rather_than_an_empty_report() {
    let unknown: crate::shared::Freshness =
        serde_json::from_str(r#"{"group_id":"group","nodes":[]}"#).unwrap();
    assert!(unknown.unpublished_nodes.is_none());
    let reported: crate::shared::Freshness =
        serde_json::from_str(r#"{"group_id":"group","nodes":[],"unpublished_nodes":[]}"#).unwrap();
    assert_eq!(reported.unpublished_nodes, Some(vec![]));
    assert!(
        !serde_json::to_string(&unknown)
            .unwrap()
            .contains("unpublished_nodes")
    );
}
