//! Executable boundary for future collection-level construction settings.
use super::*;

#[derive(Serialize)]
struct Update {
    hnsw_config: Topology,
}
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Topology {
    m: usize,
    ef_construct: usize,
}
#[derive(Deserialize)]
struct Response {
    result: Info,
}
#[derive(Deserialize)]
struct Info {
    config: Settings,
}
#[derive(Deserialize)]
struct Settings {
    hnsw_config: Topology,
}

async fn topology(fixture: &Fixture) -> Topology {
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
    response.result.config.hnsw_config
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_rejects_incompatible_physical_graph_without_resetting_or_republishing_history() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(
            &fixture.identity,
            fixture.file(1, "physical graph evidence"),
        )
        .await
        .unwrap();
    let before =
        serde_json::to_vec(&fixture.store.load(&fixture.identity).await.unwrap().data).unwrap();
    let state = fixture.store.state(&fixture.identity).await.unwrap();
    assert_eq!(
        topology(&fixture).await,
        Topology {
            m: 16,
            ef_construct: 200
        }
    );
    let update = Update {
        hnsw_config: Topology {
            m: 32,
            ef_construct: 400,
        },
    };
    reqwest::Client::new()
        .patch(format!(
            "{}/collections/{}",
            fixture.url,
            collection(&fixture.identity.group_id, false)
        ))
        .json(&update)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    for policy in [VectorIndex::Exact {}, VectorIndex::Hnsw { ef_runtime: 80 }] {
        let error = Store::connect(
            &fixture.url,
            None,
            std::slice::from_ref(&fixture.identity.group_id),
            4,
            64 * 1024 * 1024,
            policy,
            MemoryTier::Cached,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
        assert_eq!(topology(&fixture).await, update.hnsw_config);
        assert_eq!(fixture.store.state(&fixture.identity).await.unwrap(), state);
        assert_eq!(
            serde_json::to_vec(&fixture.store.load(&fixture.identity).await.unwrap().data).unwrap(),
            before
        );
    }
    fixture.cleanup().await;
}
