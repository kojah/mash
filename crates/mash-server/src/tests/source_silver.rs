use super::*;
use crate::silver::certificate::{Certificate, Completeness, INTERPRETATION_VERSION};

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_source_silver_lifecycle() {
    contract(Service::qdrant(384).await).await;
}

fn certificate() -> Certificate {
    Certificate {
        revision: 42,
        interpretation_version: INTERPRETATION_VERSION,
        completeness: Completeness::Complete,
        rejected_sources: 0,
    }
}

async fn contract(service: Service) {
    let mut events = snapshot(1);
    events[0]["source_silver"] = serde_json::to_value(certificate()).unwrap();
    // Certificates never authorize an unfinished publication.
    assert_eq!(
        service
            .upload(FIRST, &events[..events.len() - 1])
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(service.state().await.source_silver.is_none());
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::OK
    );
    let accepted = service.state().await;
    assert_eq!(accepted.source_silver, Some(certificate()));
    assert_eq!(accepted.revision, 1); // Transport and silver revisions are distinct.

    let mut retry = events.clone();
    retry[0]["source_silver"]["revision"] = json!(999);
    assert_eq!(service.upload(FIRST, &retry).await.status(), StatusCode::OK);
    assert_eq!(service.state().await.source_silver, Some(certificate()));

    let mut invalid = events.clone();
    invalid[0]["revision"] = json!(2);
    invalid[0]["source_silver"]["interpretation_version"] = json!(INTERPRETATION_VERSION + 1);
    assert_eq!(
        service.upload(FIRST, &invalid).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(service.state().await.source_silver, Some(certificate()));

    // View can become unavailable without changing the source interpretation evidence.
    let mut no_vectors = events.clone();
    no_vectors[0]["revision"] = json!(2);
    for record in no_vectors[2]["records"].as_array_mut().unwrap() {
        record["vector"] = Value::Null;
        record["generation"] = Value::Null;
    }
    assert_eq!(
        service.upload(FIRST, &no_vectors).await.status(),
        StatusCode::OK
    );
    assert_eq!(service.state().await.source_silver, Some(certificate()));

    // A delta without a certificate revokes the node's certificate.
    let mut delta = events[0].clone();
    delta["snapshot"] = json!(false);
    delta["revision"] = json!(3);
    delta["base_revision"] = json!(2);
    delta.as_object_mut().unwrap().remove("source_silver");
    assert_eq!(
        service
            .upload(FIRST, &[delta, json!({"type":"finish"})])
            .await
            .status(),
        StatusCode::OK
    );
    assert!(service.state().await.source_silver.is_none());

    events[0]["revision"] = json!(4);
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::OK
    );
    service
        .upload(
            FIRST,
            &[
                json!({"type":"start","instance":"snapshot-instance","revision":5,"base_revision":4,"snapshot":false,"forgets":["conversation"]}),
                json!({"type":"finish"}),
            ],
        )
        .await
        .error_for_status()
        .unwrap();
    assert!(service.state().await.source_silver.is_none());
    assert_eq!(
        service.upload(FIRST, &snapshot(6)).await.status(),
        StatusCode::OK
    );
    assert!(service.state().await.source_silver.is_none());
    service.cleanup().await;
}
