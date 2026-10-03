use super::*;
use crate::shared::protocol::{Batch, ConversationRecord, Event};
use crate::silver::enrichment::{TimestampOrigin, Timestamps};

#[derive(Clone, Copy)]
enum Field {
    Title,
    Project,
    Repository,
    Created,
    Updated,
}

fn conversation(events: &mut [Event]) -> &mut ConversationRecord {
    let Event::Batch(Batch::Conversations { records }) = &mut events[1] else {
        panic!("conversation fixture")
    };
    &mut records[0]
}

fn snapshot_with_evidence(revision: i64, evidence: Option<Evidence>) -> Vec<Event> {
    let mut events = snapshot(revision)
        .into_iter()
        .map(|event| serde_json::from_value(event).unwrap())
        .collect::<Vec<Event>>();
    conversation(&mut events).enrichment = evidence;
    events
}

fn remove_origin(record: &mut ConversationRecord, field: Field) {
    let evidence = record.enrichment.as_mut().unwrap();
    match field {
        Field::Title => evidence.title = None,
        Field::Project => evidence.project = None,
        Field::Repository => evidence.repository = None,
        Field::Created => evidence.timestamps.as_mut().unwrap().created_at = None,
        Field::Updated => evidence.timestamps.as_mut().unwrap().updated_at = None,
    }
}

fn remove_value(record: &mut ConversationRecord, field: Field) {
    match field {
        Field::Title => record.title = None,
        Field::Project => record.project = None,
        Field::Repository => record.repository = None,
        Field::Created => record.created_at = None,
        Field::Updated => record.updated_at = None,
    }
}

async fn publish(service: &Service, events: &[Event]) -> reqwest::Response {
    let encoded = events
        .iter()
        .map(|event| serde_json::to_value(event).unwrap())
        .collect::<Vec<_>>();
    service.upload(FIRST, &encoded).await
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_metadata_presence_rejects_contradictions_without_publication_and_accepts_legacy_absence()
 {
    let service = Service::qdrant(384).await;
    let mut unknown = evidence();
    unknown.title = Some(Origin::Unknown);
    unknown.project = Some(Origin::Unknown);
    unknown.repository = Some(Origin::Unknown);
    unknown.timestamps = Some(Timestamps {
        created_at: Some(TimestampOrigin::Unknown),
        updated_at: Some(TimestampOrigin::Unknown),
    });
    let events = snapshot_with_evidence(1, Some(unknown.clone()));
    assert_eq!(publish(&service, &events).await.status(), StatusCode::OK);
    let before = service.get("freshness", FIRST).await;
    for field in [
        Field::Title,
        Field::Project,
        Field::Repository,
        Field::Created,
        Field::Updated,
    ] {
        for origin_missing in [false, true] {
            let mut invalid = snapshot_with_evidence(2, Some(unknown.clone()));
            let record = conversation(&mut invalid);
            if origin_missing {
                remove_origin(record, field);
            } else {
                remove_value(record, field);
            }
            assert_eq!(
                publish(&service, &invalid).await.status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(service.get("freshness", FIRST).await, before);
            assert_eq!(
                metadata(&service).await.threads[0].metadata.enrichment,
                Some(unknown.clone())
            );
        }
    }
    // Empty strings remain present values; unknown provenance is still explicit.
    let mut empty_text = snapshot_with_evidence(2, Some(unknown.clone()));
    conversation(&mut empty_text).title = Some(String::new());
    assert_eq!(
        publish(&service, &empty_text).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        metadata(&service).await.threads[0]
            .metadata
            .title
            .as_deref(),
        Some("")
    );
    // Whole legacy evidence and its additive timestamp container may be absent.
    for (counter, proof) in [
        (3, None),
        (
            4,
            Some({
                let mut old = unknown.clone();
                old.timestamps = None;
                old
            }),
        ),
    ] {
        let legacy = snapshot_with_evidence(counter, proof.clone());
        assert_eq!(publish(&service, &legacy).await.status(), StatusCode::OK);
        assert_eq!(
            metadata(&service).await.threads[0].metadata.enrichment,
            proof
        );
    }
    let mut absent = snapshot_with_evidence(5, Some(unknown));
    let record = conversation(&mut absent);
    for field in [
        Field::Title,
        Field::Project,
        Field::Repository,
        Field::Created,
        Field::Updated,
    ] {
        remove_origin(record, field);
        remove_value(record, field);
    }
    assert_eq!(publish(&service, &absent).await.status(), StatusCode::OK);
    let result = metadata(&service).await.threads[0].metadata.clone();
    assert!(result.title.is_none() && result.project.is_none() && result.repository.is_none());
    assert!(result.created_at.is_none() && result.updated_at.is_none());
    service.cleanup().await;
}
