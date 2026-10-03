//! First-party native-written histories through direct ingestion and shared stores.
use super::*;
use crate::shared::protocol::{Candidates, Search, View};
use crate::{
    provider::Provider,
    silver::{DirectScan, IndexOptions, direct_scans},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    native_written_envelope_base64: String,
}

async fn contract(service: Service) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join("40000000-0000-4000-8000-000000000001.pb");
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../../docs/fixtures/windsurf-native-runtime-3.10.31.json"
    ))
    .unwrap();
    std::fs::write(
        &path,
        STANDARD
            .decode(fixture.native_written_envelope_base64)
            .unwrap(),
    )
    .unwrap();
    let options = IndexOptions {
        providers: [(Provider::Windsurf, vec![path])].into(),
        since_days: None,
        roots_are_authoritative: true,
    };
    let scan = direct_scans(&options).unwrap().remove(0);
    let source = &scan.sources[0];
    let conversation =
        DirectScan::parse_source(Provider::Windsurf, source, &source.stamp().unwrap())
            .unwrap()
            .conversation
            .unwrap();
    scan.verify().unwrap();
    let (events, ids) = super::native::publication(conversation);
    assert_eq!(ids.len(), 6);
    let response = service.upload(FIRST, &events).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().await.unwrap()
    );
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    for (query, expected) in [("cbioportal", 3), ("reasoning", 0)] {
        let search = Search {
            intent: crate::gold::exact::Intent::RelatedDiscussion,
            vector_policy: None,
            unit: crate::search_results::SearchUnit::Messages,
            query: query.into(),
            generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
            vector: vector.clone(),
            provider: Some(Provider::Windsurf),
            days: None,
            exclude_self: false,
            filter: None,
        };
        let result: Candidates = service
            .request("search", FIRST)
            .json(&search)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(result.lexical.len(), expected);
        assert_eq!(result.semantic.len(), 5);
        assert!(result.semantic.iter().all(|m| m.id != ids[5]));
        if expected > 0 {
            assert!(result.lexical.iter().any(|m| m.id == ids[0]));
            assert!(result.lexical.iter().any(|m| m.id == ids[2]));
        }
    }
    let view = View {
        node_id: "first".into(),
        id: ids[5].clone(),
        context: 0,
        content: crate::storage::MessageContent::Canonical,
    };
    let messages: Vec<crate::storage::Message> = service
        .request("view", FIRST)
        .json(&view)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0]
            .content
            .contains("Reasoning without visible response")
    );
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_native_windsurf_publication_preserves_prose_and_context() {
    contract(Service::qdrant(384).await).await;
}
