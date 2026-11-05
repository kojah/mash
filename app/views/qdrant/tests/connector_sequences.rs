//! Generated sequences at the durable point-write / digest-write boundary.
use super::*;
use crate::connectors::{self, Change, Object, Role, Version};
use proptest::prelude::*;

#[derive(Clone, Debug)]
struct Operation {
    value: u8,
    interrupt: bool,
}

fn batch(step: usize, value: u8) -> connectors::Batch {
    let id = "note".to_owned().try_into().unwrap();
    let change = if value == 0 {
        Change::DeleteObject { id }
    } else {
        Change::UpsertObject(Object {
            id,
            role: Role::Document,
            kind: "notes:note".to_owned().try_into().unwrap(),
            parent: None,
            title: None,
            content: Some(format!("needle value-{value}")),
            author: None,
            reply_to: None,
            url: None,
            // Repeated content still changes projected metadata on each delivery.
            created_at: Some(i64::try_from(step).unwrap()),
            updated_at: Some(i64::try_from(step).unwrap()),
            metadata: None,
            attachments: Vec::new(),
        })
    };
    connectors::Batch {
        schema_version: Version,
        source_id: "notes".to_owned().try_into().unwrap(),
        batch_id: format!("batch-{step}").try_into().unwrap(),
        expected_revision: step as u64,
        changes: vec![change],
        trigger: None,
        modes: None,
    }
}

async fn exercise(sequence: &[Operation]) -> Result<(), TestCaseError> {
    exercise_mode(sequence, false).await
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_connector_metadata_only_updates_refresh_search_evidence() {
    let fixture = fixture(false).await;
    let mut observed = Vec::new();
    for (revision, timestamp) in [(0, 100), (1, 200)] {
        let mut input = batch(revision, 1);
        let Change::UpsertObject(object) = &mut input.changes[0] else {
            unreachable!()
        };
        object.created_at = Some(timestamp);
        object.updated_at = Some(timestamp + 1);
        assert!(
            fixture
                .server
                .ingest_connector(&fixture.identity, input)
                .await
                .unwrap()
                .views_ready
        );
        let points = fixture
            .store
            .api
            .retrieve::<ConnectorDocument>(
                &collection(&fixture.identity.group_id, false),
                &[connector_id(
                    &fixture.identity.node_id,
                    "connector#document#notes#note",
                )],
            )
            .await
            .unwrap();
        assert_eq!(points.len(), 1);
        observed.push((
            points[0].payload.created_at,
            points[0].payload.unit.chat_updated_at,
        ));
    }
    fixture.cleanup().await;
    assert_eq!(observed, [(Some(100), Some(101)), (Some(200), Some(201))]);
}

async fn fixture(dense: bool) -> Fixture {
    let mut fixture = Fixture::new(VectorIndex::default()).await;
    fixture.store = fixture
        .store
        .clone()
        .with_build(crate::views::config::Build {
            representations: if dense {
                crate::views::config::Representations::LexicalAndDense
            } else {
                crate::views::config::Representations::LexicalOnly
            },
            ..Default::default()
        });
    fixture
        .store
        .select_views(&fixture.identity, true)
        .await
        .unwrap();
    fixture.server = fixture.serve(fixture.store.clone());
    if dense {
        static ENDPOINT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        fixture.server = fixture
            .server
            .with_embedder(crate::shared::server::store::Embedder {
                models: std::path::PathBuf::new(),
                remote: Some(crate::semantic::remote::Configuration {
                    url: ENDPOINT.get_or_init(recovery::embeddings::serve).clone(),
                    model: "test".into(),
                    dimensions: 4,
                    api: crate::semantic::remote::Api::OpenaiCompatible,
                    token_env: None,
                    request_dimensions: true,
                    redaction: crate::redaction::Policy::ON,
                }),
            });
    }
    fixture
        .server
        .index(
            &fixture.identity,
            Fixture::upload(&[Event::Start(Fixture::start(1)), Event::Finish]),
        )
        .await
        .unwrap();
    fixture
}

async fn exercise_mode(sequence: &[Operation], dense: bool) -> Result<(), TestCaseError> {
    let fixture = fixture(dense).await;
    let mut failure = None;
    for (step, operation) in sequence.iter().enumerate() {
        let server = if operation.interrupt {
            fixture.server.clone().with_connector_interruption()
        } else {
            fixture.server.clone()
        };
        let receipt = server
            .ingest_connector(&fixture.identity, batch(step, operation.value))
            .await
            .unwrap();
        assert_eq!(receipt.source_revision, step as u64 + 1);
        if operation.interrupt {
            continue;
        }
        let points = fixture
            .store
            .api
            .retrieve::<ConnectorDocument>(
                &collection(&fixture.identity.group_id, false),
                &[connector_id(
                    &fixture.identity.node_id,
                    "connector#document#notes#note",
                )],
            )
            .await
            .unwrap();
        let expected = (operation.value != 0).then(|| {
            (
                format!("needle value-{}", operation.value),
                Some(i64::try_from(step).unwrap()),
            )
        });
        let actual = points
            .first()
            .map(|p| (p.payload.unit.content.clone(), p.payload.created_at));
        let current = fixture
            .server
            .freshness(&fixture.identity)
            .await
            .unwrap()
            .nodes[0]
            .lexical_current;
        if actual != expected || !current {
            failure = Some(TestCaseError::fail(format!(
                "step {step}: expected {expected:?}, physical {actual:?}, current {current}"
            )));
            break;
        }
    }
    fixture.cleanup().await;
    failure.map_or(Ok(()), Err)
}

#[test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
fn qdrant_connector_sequences_interrupted_insert_then_delete_regression() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(exercise(&[
            Operation {
                value: 1,
                interrupt: true,
            },
            Operation {
                value: 0,
                interrupt: false,
            },
        ]))
        .unwrap();
}

#[test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
fn qdrant_connector_sequences_interrupted_delete_then_restore_regression() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(exercise(&[
            Operation {
                value: 2,
                interrupt: false,
            },
            Operation {
                value: 0,
                interrupt: true,
            },
            Operation {
                value: 2,
                interrupt: false,
            },
        ]))
        .unwrap();
}

#[test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
fn qdrant_generated_connector_sequences_agree_with_silver_after_recovery() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let mut config = proptest::test_runner::Config::default();
    if std::env::var_os("PROPTEST_CASES").is_none() {
        config.cases = 16;
    }
    config.max_shrink_iters = 32;
    config.failure_persistence = None;
    let operations =
        (0u8..3, any::<bool>()).prop_map(|(value, interrupt)| Operation { value, interrupt });
    proptest::test_runner::TestRunner::new(config)
        .run(
            &(any::<bool>(), prop::collection::vec(operations, 1..5)),
            |(dense, sequence)| runtime.block_on(exercise_mode(&sequence, dense)),
        )
        .unwrap();
}

#[test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
fn qdrant_connector_sequences_dense_updates_and_recovery() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(exercise_mode(
            &[
                Operation {
                    value: 1,
                    interrupt: false,
                },
                Operation {
                    value: 2,
                    interrupt: true,
                },
                Operation {
                    value: 1,
                    interrupt: false,
                },
            ],
            true,
        ))
        .unwrap();
}
