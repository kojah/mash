use super::*;
use crate::silver::enrichment::{Evidence, Origin, SourceField, VERSION};

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_enrichment_catalog_search_replacement_and_validation() {
    contract(Service::qdrant(384).await).await;
}

fn evidence() -> Evidence {
    Evidence {
        version: VERSION,
        title: Some(Origin::FirstVisibleUserMessage {
            message_id: "searchable".into(),
            rule_version: VERSION,
        }),
        project: Some(Origin::SourceMetadata {
            field: SourceField::Cwd,
        }),
        repository: Some(Origin::Unknown),
    }
}

async fn metadata(service: &Service) -> crate::thread_catalog::Page {
    service
        .request("threads", FIRST)
        .json(&crate::thread_catalog::Request::default())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn contract(service: Service) {
    let mut events = snapshot(1);
    let mut batch: protocol::Event = serde_json::from_value(events[1].clone()).unwrap();
    let protocol::Event::Batch(protocol::Batch::Conversations { records }) = &mut batch else {
        panic!("conversation fixture")
    };
    records[0].title = Some("hello shared history".into());
    records[0].enrichment = Some(evidence());
    events[1] = serde_json::to_value(batch).unwrap();
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        metadata(&service).await.threads[0].metadata.enrichment,
        Some(evidence())
    );
    let mut vector = vec![0.0; 384];
    vector[0] = 1.0;
    let found: protocol::Candidates = service
        .request("search", FIRST)
        .json(&json!({"query":"hello","vector":vector}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!found.lexical.is_empty());
    assert!(!found.semantic.is_empty());
    assert!(
        found
            .lexical
            .iter()
            .chain(&found.semantic)
            .all(|hit| hit.metadata.enrichment == Some(evidence()))
    );
    let mut rejected = events.clone();
    rejected[0]["revision"] = json!(2);
    rejected[1]["records"][0]["enrichment"]["version"] = json!(VERSION + 1);
    assert_eq!(
        service.upload(FIRST, &rejected).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        metadata(&service).await.threads[0].metadata.enrichment,
        Some(evidence())
    );
    // Old records have no evidence; replacement must clear stale annotations.
    assert_eq!(
        service.upload(FIRST, &snapshot(2)).await.status(),
        StatusCode::OK
    );
    assert!(
        metadata(&service).await.threads[0]
            .metadata
            .enrichment
            .is_none()
    );
    service.cleanup().await;
}
