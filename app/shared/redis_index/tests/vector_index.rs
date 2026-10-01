use super::catalog_tests::Fixture;
use super::vector_index::{FilterPolicy, VectorIndex};
use super::*;
use crate::search_filter::Filter;

fn hnsw(filter_policy: FilterPolicy, batch_size: Option<usize>) -> VectorIndex {
    VectorIndex::Hnsw {
        m: 16,
        ef_construction: 200,
        ef_runtime: 200,
        filter_policy,
        batch_size,
    }
}

#[test]
fn redis_vector_configuration_is_strict_and_bounded() {
    for valid in [
        "algorithm = 'exact'",
        "algorithm = 'hnsw'",
        "algorithm = 'hnsw'\nfilter_policy = 'batches'\nbatch_size = 512",
    ] {
        toml::from_str::<VectorIndex>(valid)
            .unwrap()
            .validate()
            .unwrap();
    }
    for invalid in [
        "algorithm = 'flat'",
        "algorithm = 'exact'\nm = 16",
        "algorithm = 'hnsw'\nunknown = 1",
        "algorithm = 'hnsw'\nm = 0",
        "algorithm = 'hnsw'\nm = 129",
        "algorithm = 'hnsw'\nef_runtime = 0",
        "algorithm = 'hnsw'\nef_runtime = 4097",
        "algorithm = 'hnsw'\nef_construction = 3",
        "algorithm = 'hnsw'\nef_construction = 4097",
        "algorithm = 'hnsw'\nfilter_policy = 'invalid'",
        "algorithm = 'hnsw'\nbatch_size = 128",
        "algorithm = 'hnsw'\nfilter_policy = 'exact'\nbatch_size = 128",
        "algorithm = 'hnsw'\nfilter_policy = 'batches'\nbatch_size = 0",
        "algorithm = 'hnsw'\nfilter_policy = 'batches'\nbatch_size = 4097",
    ] {
        assert!(
            !toml::from_str::<VectorIndex>(invalid).is_ok_and(|index| index.validate().is_ok()),
            "accepted {invalid}"
        );
    }
}

#[test]
fn redis_vector_schema_decoder_rejects_missing_duplicate_and_malformed_fields() {
    fn string(value: &str) -> redis::Value {
        redis::Value::BulkString(value.as_bytes().to_vec())
    }
    fn info(fields: Vec<redis::Value>) -> redis::Value {
        redis::Value::Array(vec![
            string("attributes"),
            redis::Value::Array(vec![redis::Value::Array(fields)]),
            string("indexing"),
            redis::Value::Int(0),
        ])
    }
    let fields = [
        ("identifier", "vector"),
        ("type", "VECTOR"),
        ("algorithm", "FLAT"),
        ("data_type", "FLOAT32"),
        ("distance_metric", "COSINE"),
        ("dim", "384"),
    ]
    .into_iter()
    .flat_map(|(key, value)| [string(key), string(value)])
    .collect::<Vec<_>>();
    let exact = VectorIndex::default();
    exact.verify(&info(fields.clone()), 384).unwrap();
    assert!(exact.verify(&info(fields.clone()), 768).is_err());
    assert!(
        hnsw(FilterPolicy::Auto, None)
            .verify(&info(fields.clone()), 384)
            .is_err()
    );
    let mut duplicate = fields.clone();
    duplicate.extend([string("dim"), string("384")]);
    let mut odd = fields.clone();
    odd.push(string("trailing"));
    let mut malformed = fields.clone();
    *malformed.last_mut().unwrap() = redis::Value::Nil;
    for invalid in [
        info(vec![]),
        info(duplicate),
        info(odd),
        info(malformed),
        redis::Value::Nil,
    ] {
        assert!(exact.verify(&invalid, 384).is_err());
    }
    assert!(!super::vector_index::indexing(&info(fields)).unwrap());
    assert!(super::vector_index::indexing(&redis::Value::Nil).is_err());
}

fn input(provider: Provider, exclude_self: bool) -> Search {
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    Search {
        unit: crate::search_results::SearchUnit::Messages,
        query: "private".into(),
        generation: semantic::SHARED_EMBEDDING_GENERATION.into(),
        vector,
        provider: Some(provider),
        days: None,
        exclude_self,
        filter: None,
    }
}

async fn verify_metadata_filters(fixture: &Fixture) {
    let mut scoped = input(Provider::Codex, false);
    scoped.filter = Some(Filter::All(vec![
        Filter::Project(vec!["/project".into()]),
        Filter::Repository(vec!["repo://project".into()]),
        Filter::Node(vec!["node".into()]),
        Filter::Role(vec![crate::role::Role::User]),
        Filter::Conversation(vec!["thread".into()]),
        Filter::After(0),
        Filter::Before(2),
    ]));
    assert_eq!(
        fixture
            .store
            .search(&fixture.identity, scoped)
            .await
            .unwrap()
            .semantic
            .len(),
        1
    );
    let mut excluded = input(Provider::Codex, false);
    excluded.filter = Some(Filter::Not(Box::new(Filter::Project(vec![
        "/project".into(),
    ]))));
    assert!(
        fixture
            .store
            .search(&fixture.identity, excluded)
            .await
            .unwrap()
            .semantic
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_hnsw_policies_preserve_filters_ownership_replacement_and_deletion() {
    for index in [
        hnsw(FilterPolicy::Auto, None),
        hnsw(FilterPolicy::Exact, None),
        hnsw(FilterPolicy::Batches, None),
        hnsw(FilterPolicy::Batches, Some(128)),
    ] {
        let fixture = Fixture::with_vector_index(index).await;
        fixture.upload(1, "first").await.unwrap();
        let found = fixture
            .store
            .search(&fixture.identity, input(Provider::Codex, false))
            .await
            .unwrap();
        assert_eq!(found.semantic.len(), 1);
        assert_eq!(found.semantic[0].id, "message");
        assert_eq!(found.semantic[0].node_id.as_deref(), Some("node"));
        verify_metadata_filters(&fixture).await;
        assert!(
            fixture
                .store
                .search(&fixture.identity, input(Provider::ClaudeCode, false))
                .await
                .unwrap()
                .semantic
                .is_empty()
        );
        assert!(
            fixture
                .store
                .search(&fixture.identity, input(Provider::Codex, true))
                .await
                .unwrap()
                .semantic
                .is_empty()
        );
        let mut reader = fixture.identity.clone();
        reader.node_id = "other-reader".into();
        assert_eq!(
            fixture
                .store
                .search(&reader, input(Provider::Codex, true))
                .await
                .unwrap()
                .semantic
                .len(),
            1
        );
        fixture.upload(2, "replacement").await.unwrap();
        assert_eq!(
            fixture
                .store
                .search(&fixture.identity, input(Provider::Codex, false))
                .await
                .unwrap()
                .semantic
                .len(),
            1
        );
        fixture
            .store
            .forget(
                &fixture.identity,
                Forget {
                    instance: "catalog-instance".into(),
                    revision: Some(3),
                    ids: vec!["thread".into()],
                },
            )
            .await
            .unwrap();
        assert!(
            fixture
                .store
                .search(&fixture.identity, input(Provider::Codex, false))
                .await
                .unwrap()
                .semantic
                .is_empty()
        );
        fixture.cleanup().await;
    }
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_vector_schema_changes_fail_without_replacing_published_data() {
    let index = hnsw(FilterPolicy::Auto, None);
    let fixture = Fixture::with_vector_index(index).await;
    fixture.upload(1, "retained").await.unwrap();
    let url = std::env::var("MASH_TEST_REDIS_URL").unwrap();
    let group = std::slice::from_ref(&fixture.identity.group_id);
    for mismatch in [
        VectorIndex::Exact {},
        VectorIndex::Hnsw {
            m: 32,
            ef_construction: 200,
            ef_runtime: 200,
            filter_policy: FilterPolicy::Auto,
            batch_size: None,
        },
        VectorIndex::Hnsw {
            m: 16,
            ef_construction: 100,
            ef_runtime: 200,
            filter_policy: FilterPolicy::Auto,
            batch_size: None,
        },
    ] {
        let Err(error) = Store::connect(&url, group, 384, 32 * 1024 * 1024, mismatch).await else {
            panic!("accepted mismatched vector schema");
        };
        assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
        assert!(error.error.message.contains("new group_id"));
        assert_eq!(
            fixture
                .store
                .status(&fixture.identity)
                .await
                .unwrap()
                .messages,
            1
        );
    }
    let runtime_changed = VectorIndex::Hnsw {
        m: 16,
        ef_construction: 200,
        ef_runtime: 50,
        filter_policy: FilterPolicy::Batches,
        batch_size: None,
    };
    let reopened = Store::connect(&url, group, 384, 32 * 1024 * 1024, runtime_changed)
        .await
        .unwrap();
    assert_eq!(
        reopened
            .search(&fixture.identity, input(Provider::Codex, false))
            .await
            .unwrap()
            .semantic
            .len(),
        1
    );
    fixture.cleanup().await;
    let exact = Fixture::with_vector_index(VectorIndex::Exact {}).await;
    assert!(
        Store::connect(
            &url,
            std::slice::from_ref(&exact.identity.group_id),
            384,
            32 * 1024 * 1024,
            index
        )
        .await
        .is_err()
    );
    exact.cleanup().await;
}
