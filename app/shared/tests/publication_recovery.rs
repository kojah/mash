use super::super::publication::target as publication_target;
use super::*;
use axum::{Json, Router, extract::State, routing::get};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Fixture {
    target: reuse::Target,
    freshness: Freshness,
    plans: AtomicUsize,
    change_head: bool,
}

async fn plan(State(fixture): State<Arc<Fixture>>) -> Json<reuse::Target> {
    let mut target = fixture.target.clone();
    if fixture.plans.fetch_add(1, Ordering::SeqCst) > 0 && fixture.change_head {
        target.publication_version.push_str("-changed");
    }
    Json(target)
}

async fn freshness(State(fixture): State<Arc<Fixture>>) -> Json<Freshness> {
    // Exercise the same typed wire boundary as the real server.
    Json(serde_json::from_slice(&serde_json::to_vec(&fixture.freshness).unwrap()).unwrap())
}

fn current_node(target: &reuse::Target) -> NodeFreshness {
    let silver = silver_revision::State::next(None, "a".repeat(64)).unwrap();
    NodeFreshness {
        artifacts: None,
        lexical_manifest: Some(lexical_manifest::Manifest::new(silver.clone())),
        lexical_current: true,
        gold_manifest: Some(gold_manifest::Manifest::new(
            target.build.clone(),
            target.generation.clone(),
            silver.clone(),
        )),
        gold_current: true,
        silver_coverage: vec![],
        silver_revision: Some(silver),
        node_id: "local-node".into(),
        revision: target.publication_revision,
        last_synced_at: None,
        last_updated_at: None,
        conversations: 1,
        messages: 2,
    }
}

fn configuration(address: std::net::SocketAddr) -> SharedConfig {
    SharedConfig {
        group_id: "test".into(),
        node_id: "local-node".into(),
        api_url: format!("http://{address}"),
        token_env: "HOME".into(),
    }
}

fn current_freshness(target: &reuse::Target, node: NodeFreshness) -> Freshness {
    Freshness {
        group_id: "test".into(),
        unpublished_nodes: None,
        group_gold: Some(Box::new(GroupGold {
            version: "group".into(),
            build: target.build.clone(),
            generation: target.generation.clone(),
            dimensions: target.dimensions,
            vector_search: qdrant_index::VectorIndex::default(),
        })),
        nodes: vec![node],
    }
}

#[tokio::test]
async fn lost_ack_recovers_only_with_stable_source_and_current_artifacts() {
    for case in [
        "current",
        "stale-gold",
        "stale-lexical",
        "changed-model",
        "changed-build",
        "incomplete-source",
        "changed-head",
        "edited",
        "other-owner",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.sqlite3");
        seed_ready(&path, semantic::SHARED_EMBEDDING_GENERATION);
        let storage = Storage::open_existing(&path).unwrap();
        let mut target = reuse::Target {
            schema_version: 1,
            build: crate::gold::config::Build::default(),
            generation: semantic::SHARED_EMBEDDING_GENERATION.into(),
            dimensions: 384,
            publication_version: "head".into(),
            instance: None,
            publication_revision: 0,
            threads: std::collections::BTreeMap::new(),
        };
        storage
            .with_publication_snapshot(|snapshot| {
                assert!(snapshot.source_silver.is_complete());
                target.instance = Some(snapshot.instance.clone());
                target.publication_revision = snapshot.revision;
                Ok(())
            })
            .unwrap();
        drop(storage);
        let mut node = current_node(&target);
        match case {
            "stale-gold" => node.gold_manifest = None,
            "stale-lexical" => node.lexical_current = false,
            "changed-model" => {
                node.gold_manifest.as_mut().unwrap().model_generation = "other".into();
            }
            "changed-build" => {
                node.gold_manifest.as_mut().unwrap().build.representations =
                    crate::gold::config::Representations::LexicalOnly;
            }
            "incomplete-source" => {
                let mut writer = Storage::open_writer(&path).unwrap();
                writer
                    .begin_silver_scan(crate::provider::Provider::Codex)
                    .unwrap();
                writer.commit_writer().unwrap();
            }
            "edited" => target.publication_revision -= 1,
            "other-owner" => target.instance = Some("another-node".into()),
            _ => {}
        }
        let fixture = Arc::new(Fixture {
            freshness: current_freshness(&target, node),
            target,
            plans: AtomicUsize::new(0),
            change_head: case == "changed-head",
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = configuration(listener.local_addr().unwrap());
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/publication-plan", get(plan))
                    .route("/v1/freshness", get(freshness))
                    .with_state(fixture.clone()),
            )
            .into_future(),
        );
        assert!(Storage::publication_is_pending(&path, &publication_target(&config)).unwrap());
        let publication = publish(&path, &config, None).await;
        if case == "current" {
            assert_eq!(publication.state, PublicationState::Published);
            let work = publication.preparation.unwrap();
            assert_eq!(work.upload_bytes, 0);
            assert_eq!(work.model_inferences, 0);
            assert!(!Storage::publication_is_pending(&path, &publication_target(&config)).unwrap());
            assert_eq!(fixture.plans.load(Ordering::SeqCst), 2);
        } else {
            // The mock deliberately has no upload endpoint: stale/racing state
            // must attempt normal publication or reject ownership, never ack.
            assert_eq!(publication.state, PublicationState::Pending, "{case}");
            assert!(
                Storage::publication_is_pending(&path, &publication_target(&config)).unwrap(),
                "{case}"
            );
        }
        server.abort();
    }
}
