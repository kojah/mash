use super::*;
use crate::silver::enrichment::{Evidence, Origin, SourceField, VERSION};
mod presence;

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
    let expected = Evidence {
        version: VERSION,
        timestamps: None,
        title: Some(Origin::SourceMetadata {
            field: SourceField::SessionTitle,
        }),
        project: Some(Origin::SourceMetadata {
            field: SourceField::SessionDirectory,
        }),
        repository: Some(Origin::SourceMetadata {
            field: SourceField::ProjectWorktree,
        }),
    };
    contract(
        Service::qdrant(384).await,
        crate::provider::Provider::Opencode,
        expected,
    )
    .await;
}

fn evidence() -> Evidence {
    Evidence {
        version: VERSION,
        timestamps: None,
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

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_selected_title_origins_round_trip_and_reject_invalid_message_references() {
    use crate::provider::Provider;
    for (provider, origin) in [
        (
            Provider::Opencode,
            Origin::SourceMetadata {
                field: SourceField::SessionTitle,
            },
        ),
        (
            Provider::Codex,
            Origin::FirstVisibleUserMessage {
                message_id: "searchable".into(),
                rule_version: VERSION,
            },
        ),
    ] {
        let mut expected = evidence();
        expected.title = Some(origin);
        contract(Service::qdrant(384).await, provider, expected).await;
    }
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_timestamp_evidence_round_trips_and_rejects_invalid_rules_and_ids() {
    use crate::silver::enrichment::{Boundary, TimestampField, TimestampOrigin, Timestamps};
    for provider in [
        crate::provider::Provider::Codex,
        crate::provider::Provider::ClaudeCode,
        crate::provider::Provider::Opencode,
    ] {
        let mut expected = evidence();
        let native_fields = match provider {
            crate::provider::Provider::Opencode => Some([
                TimestampField::SessionTimeCreated,
                TimestampField::SessionTimeUpdated,
            ]),
            _ => None,
        };
        let origins = if let Some(fields) = native_fields {
            fields.map(|field| TimestampOrigin::SourceMetadata {
                field,
                rule_version: VERSION,
            })
        } else {
            [Boundary::Minimum, Boundary::Maximum].map(|selection| {
                TimestampOrigin::MessageBoundary {
                    message_id: "searchable".into(),
                    selection,
                    rule_version: VERSION,
                }
            })
        };
        let [created, updated] = origins;
        expected.timestamps = Some(Timestamps {
            created_at: Some(created),
            updated_at: Some(updated),
        });
        contract(Service::qdrant(384).await, provider, expected).await;
    }
}

async fn metadata(service: &Service) -> crate::chat_catalog::Page {
    service
        .request("chats", FIRST)
        .json(&crate::chat_catalog::Request::default())
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
    let protocol::Event::Batch(protocol::Batch::Chats { records }) = &mut batch else {
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
        metadata(&service).await.chats[0].metadata.enrichment,
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
        metadata(&service).await.chats[0].metadata.enrichment,
        Some(expected.clone())
    );
    reject_invalid_message_references(&service, &events).await;
    assert_eq!(
        metadata(&service).await.chats[0].metadata.enrichment,
        Some(expected)
    );
    // Old records have no evidence; replacement must clear stale annotations.
    let mut cleared = events;
    cleared[0]["revision"] = json!(2);
    let mut batch: protocol::Event = serde_json::from_value(cleared[1].clone()).unwrap();
    let protocol::Event::Batch(protocol::Batch::Chats { records }) = &mut batch else {
        panic!("conversation fixture")
    };
    records[0].enrichment = None;
    cleared[1] = serde_json::to_value(batch).unwrap();
    assert_eq!(
        service.upload(FIRST, &cleared).await.status(),
        StatusCode::OK
    );
    assert!(
        metadata(&service).await.chats[0]
            .metadata
            .enrichment
            .is_none()
    );
    service.cleanup().await;
}

async fn reject_invalid_message_references(service: &Service, events: &[Value]) {
    use crate::silver::enrichment::{Boundary, TimestampOrigin, Timestamps};
    for (message_id, rule_version) in [
        (String::new(), VERSION),
        ("x".repeat(4097), VERSION),
        ("searchable".into(), VERSION + 1),
    ] {
        let mut rejected = events.to_vec();
        rejected[0]["revision"] = json!(2);
        let mut batch: protocol::Event = serde_json::from_value(rejected[1].clone()).unwrap();
        let protocol::Event::Batch(protocol::Batch::Chats { records }) = &mut batch else {
            panic!("conversation fixture")
        };
        records[0].enrichment.as_mut().unwrap().timestamps = Some(Timestamps {
            created_at: Some(TimestampOrigin::MessageBoundary {
                message_id,
                selection: Boundary::Minimum,
                rule_version,
            }),
            updated_at: Some(TimestampOrigin::Unknown),
        });
        rejected[1] = serde_json::to_value(batch).unwrap();
        assert_eq!(
            service.upload(FIRST, &rejected).await.status(),
            StatusCode::BAD_REQUEST
        );
    }
}
