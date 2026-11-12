//! Check reported memory policy and preserved publications against a live service.
use super::*;

#[derive(Deserialize)]
struct Response {
    result: Collection,
}
#[derive(Deserialize)]
struct Collection {
    config: Config,
}
#[derive(Deserialize)]
struct Config {
    params: Parameters,
    hnsw_config: Placement,
}
#[derive(Deserialize)]
struct Parameters {
    vectors: Dense,
    payload: Placement,
}
#[derive(Deserialize)]
struct Dense {
    dense: Placement,
}
#[derive(Deserialize)]
struct Placement {
    memory: PhysicalMemory,
}

async fn assert_policy(fixture: &Fixture, memory: MemoryTier) {
    let response: Response = reqwest::Client::new()
        .get(format!(
            "{}/collections/{}",
            fixture.url,
            collection(&fixture.identity.group_id, false)
        ))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let config = response.result.config;
    assert_eq!(config.params.vectors.dense.memory, memory.dense());
    assert_eq!(config.hnsw_config.memory, memory.index());
    assert_eq!(config.params.payload.memory, PAYLOAD_MEMORY);
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.20+"]
async fn durable_memory_activation_preserves_publications_and_is_idempotent() {
    let fixture = Fixture::with_memory(VectorIndex::default(), MemoryTier::Cached).await;
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "original"))
        .await
        .unwrap();
    let before =
        serde_json::to_vec(&fixture.server.freshness(&fixture.identity).await.unwrap()).unwrap();
    assert!(
        fixture
            .reopen(VectorIndex::default(), MemoryTier::Durable)
            .await
            .is_err()
    );
    for _ in 0..2 {
        let upgraded = Store::connect(
            (&fixture.url, &fixture.grpc_url),
            None,
            std::slice::from_ref(&fixture.identity.group_id),
            4,
            64 * 1024 * 1024,
            VectorIndex::default(),
            MemoryPolicy {
                tier: MemoryTier::Durable,
                activate: true,
            },
        )
        .await
        .unwrap();
        let upgraded = fixture.serve(upgraded);
        assert_policy(&fixture, MemoryTier::Durable).await;
        assert_eq!(
            before,
            serde_json::to_vec(&upgraded.freshness(&fixture.identity).await.unwrap()).unwrap()
        );
        assert_eq!(
            upgraded.status(&fixture.identity).await.unwrap().messages,
            2
        );
        assert_filters_and_context(&fixture).await;
    }
    let reopened = fixture
        .reopen(VectorIndex::default(), MemoryTier::Durable)
        .await
        .unwrap();
    reopened
        .index(&fixture.identity, fixture.file(2, "changed"))
        .await
        .unwrap();
    assert_eq!(
        reopened.status(&fixture.identity).await.unwrap().messages,
        2
    );
    fixture.cleanup().await;
}

async fn assert_filters_and_context(fixture: &Fixture) {
    for (provider, expected) in [
        (crate::provider::Provider::Codex, 2),
        (crate::provider::Provider::ClaudeCode, 0),
    ] {
        let mut query = fixture.query();
        query.filter = Some(crate::search_filter::Filter::All(vec![
            crate::search_filter::Filter::Node(vec![fixture.identity.node_id.clone()]),
            crate::search_filter::Filter::Provider(vec![provider]),
        ]));
        let results = fixture
            .server
            .search(&fixture.identity, query)
            .await
            .unwrap();
        assert_eq!(results.lexical.len(), expected);
        assert_eq!(results.semantic.len(), expected);
    }
    let context = fixture
        .server
        .view(
            &fixture.identity,
            protocol::View {
                node_id: fixture.identity.node_id.clone(),
                id: "message-0".into(),
                context: 1,
                content: crate::storage::MessageContent::Canonical,
            },
        )
        .await
        .unwrap();
    assert_eq!(context.len(), 2);
    assert!(
        context
            .iter()
            .all(|message| message.content.contains("canonical original"))
    );
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.20+"]
async fn cached_and_cold_indexes_preserve_search_and_data_across_reopen_mismatch_and_forget() {
    for memory in [MemoryTier::Cached, MemoryTier::Cold, MemoryTier::Durable] {
        for index in [VectorIndex::Exact {}, VectorIndex::Hnsw { ef_runtime: 200 }] {
            let fixture = Fixture::with_memory(index, memory).await;
            fixture
                .server
                .index(&fixture.identity, fixture.file(1, "original"))
                .await
                .unwrap();
            assert_policy(&fixture, memory).await;
            assert_filters_and_context(&fixture).await;
            let freshness =
                serde_json::to_vec(&fixture.server.freshness(&fixture.identity).await.unwrap())
                    .unwrap();
            let alternate_query = match index {
                VectorIndex::Exact {} => VectorIndex::Hnsw { ef_runtime: 80 },
                VectorIndex::Hnsw { .. } => VectorIndex::Exact {},
            };
            let alternate = fixture.reopen(alternate_query, memory).await.unwrap();
            let hits = alternate
                .search(&fixture.identity, fixture.query())
                .await
                .unwrap();
            assert_eq!(hits.lexical.len(), 2);
            assert_eq!(hits.semantic.len(), 2);
            assert_eq!(
                freshness,
                serde_json::to_vec(&alternate.freshness(&fixture.identity).await.unwrap()).unwrap()
            );
            let reopened = fixture.reopen(index, memory).await.unwrap();
            assert_eq!(
                reopened.status(&fixture.identity).await.unwrap().messages,
                2
            );
            let opposite = match memory {
                MemoryTier::Cached => MemoryTier::Cold,
                MemoryTier::Cold | MemoryTier::Durable => MemoryTier::Cached,
            };
            let error = fixture.reopen(index, opposite).await.err().unwrap();
            assert!(matches!(error.error.kind, crate::ErrorKind::Configuration));
            assert_policy(&fixture, memory).await;
            let results = fixture.search().await;
            assert_eq!(results.lexical.len(), 2);
            assert_eq!(results.semantic.len(), 2);
            assert!(
                results
                    .documents
                    .iter()
                    .all(|d| d.projection.contains("original"))
            );
            assert_eq!(
                fixture
                    .server
                    .state(&fixture.identity)
                    .await
                    .unwrap()
                    .revision,
                1
            );
            reopened
                .index(&fixture.identity, Fixture::forget(2, &["thread"]))
                .await
                .unwrap();
            assert!(fixture.search().await.documents.is_empty());
            fixture.cleanup().await;
        }
    }
}
