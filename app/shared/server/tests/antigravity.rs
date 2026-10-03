use super::*;
use crate::shared::protocol::{Candidates, Search, View};
use crate::{
    provider::Provider,
    silver::{DirectScan, IndexOptions, direct_scans},
};

async fn contract(service: Service) {
    let directory = tempfile::tempdir().unwrap();
    let path = crate::silver::write_antigravity_fixture(directory.path());
    let options = IndexOptions {
        providers: [(Provider::Antigravity, vec![path])].into(),
        since_days: None,
        roots_are_authoritative: true,
    };
    let scan = direct_scans(&options).unwrap().remove(0);
    let source = &scan.sources[0];
    let conversation =
        DirectScan::parse_source(Provider::Antigravity, source, &source.stamp().unwrap())
            .unwrap()
            .conversation
            .unwrap();
    scan.verify().unwrap();
    let (events, ids) = super::native::publication(conversation);
    assert_eq!(ids.len(), 9);
    service
        .upload(FIRST, &events)
        .await
        .error_for_status()
        .unwrap();
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    for (query, expected) in [
        ("cbioportal", 9),
        ("reasoning", 0),
        ("synthetic_scope_only_marker", 0),
    ] {
        let request = Search {
            unit: crate::search_results::SearchUnit::Messages,
            query: query.into(),
            generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
            vector: vector.clone(),
            provider: Some(Provider::Antigravity),
            days: None,
            exclude_self: false,
            filter: None,
        };
        let result: Candidates = service
            .request("search", FIRST)
            .json(&request)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(result.lexical.len(), expected, "{query}");
        assert_eq!(result.semantic.len(), 9);
        assert!(
            result
                .lexical
                .iter()
                .all(|m| m.provider == Provider::Antigravity)
        );
    }
    let request = View {
        node_id: "first".into(),
        id: ids.last().unwrap().clone(),
        context: 0,
        content: crate::storage::MessageContent::Canonical,
    };
    let messages: Vec<crate::storage::Message> = service
        .request("view", FIRST)
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].content.contains("synthetic_scope_only_marker"));
    service.cleanup().await;
}
#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL with pgvector"]
async fn postgres_native_antigravity_prose_and_context() {
    contract(Service::new_backend(true, false).await).await;
}
#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_native_antigravity_prose_and_context() {
    contract(Service::new_backend(false, true).await).await;
}
