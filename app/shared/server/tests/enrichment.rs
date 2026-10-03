use super::*;
use crate::silver::enrichment::{Evidence, Origin, SourceField, VERSION};

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_enrichment_catalog_search_replacement_and_validation() {
    contract(
        Service::qdrant(384).await,
        crate::provider::Provider::Codex,
        evidence(),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_native_field_origins_round_trip_in_catalog_search_and_replacement() {
    use crate::provider::Provider;
    for (provider, title, project, repository) in [
        (
            Provider::Opencode,
            Some(SourceField::SessionTitle),
            SourceField::SessionDirectory,
            Some(SourceField::ProjectWorktree),
        ),
        (
            Provider::Hermes,
            None,
            SourceField::Cwd,
            Some(SourceField::GitRepoRoot),
        ),
        (Provider::Pi, None, SourceField::Cwd, None),
    ] {
        let expected = Evidence {
            version: VERSION,
            title: Some(title.map_or(Origin::Unknown, |field| Origin::SourceMetadata { field })),
            project: Some(Origin::SourceMetadata { field: project }),
            repository: repository.map(|field| Origin::SourceMetadata { field }),
        };
        contract(Service::qdrant(384).await, provider, expected).await;
    }
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

async fn contract(service: Service, provider: crate::provider::Provider, expected: Evidence) {
    let mut events = snapshot(1);
    let mut batch: protocol::Event = serde_json::from_value(events[1].clone()).unwrap();
    let protocol::Event::Batch(protocol::Batch::Conversations { records }) = &mut batch else {
        panic!("conversation fixture")
    };
    records[0].title = Some("hello shared history".into());
    records[0].provider = provider;
    records[0].repository = expected
        .repository
        .as_ref()
        .map(|_| "/work/repository".into());
    records[0].enrichment = Some(expected.clone());
    events[1] = serde_json::to_value(batch).unwrap();
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        metadata(&service).await.threads[0].metadata.enrichment,
        Some(expected.clone())
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
            .all(|hit| hit.metadata.enrichment.as_ref() == Some(&expected))
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
        Some(expected)
    );
    // Old records have no evidence; replacement must clear stale annotations.
    let mut cleared = events;
    cleared[0]["revision"] = json!(2);
    let mut batch: protocol::Event = serde_json::from_value(cleared[1].clone()).unwrap();
    let protocol::Event::Batch(protocol::Batch::Conversations { records }) = &mut batch else {
        panic!("conversation fixture")
    };
    records[0].enrichment = None;
    cleared[1] = serde_json::to_value(batch).unwrap();
    assert_eq!(
        service.upload(FIRST, &cleared).await.status(),
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
