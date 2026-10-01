use super::*;
use crate::provider::Provider;
use crate::shared::protocol::{Batch, Event, Start};
use std::collections::BTreeMap;
use std::io::Write;

pub(super) struct Fixture {
    pub(super) store: Store,
    pub(super) identity: Identity,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_vector_index(VectorIndex::default()).await
    }

    pub(super) async fn with_vector_index(vector_index: VectorIndex) -> Self {
        let mut random = [0; 16];
        getrandom::fill(&mut random).unwrap();
        let group = format!("catalog-test-{}", blake3::hash(&random).to_hex());
        let store = Store::connect(
            &std::env::var("MASH_TEST_REDIS_URL").unwrap(),
            std::slice::from_ref(&group),
            384,
            32 * 1024 * 1024,
            vector_index,
        )
        .await
        .unwrap();
        let identity = Identity {
            group_id: group,
            node_id: "node".into(),
            generation: semantic::SHARED_EMBEDDING_GENERATION.into(),
            dimensions: 384,
        };
        Self { store, identity }
    }

    pub(super) async fn upload(&self, revision: i64, title: &str) -> Result<Indexed, AppError> {
        let mut vector = vec![0.0; 384];
        vector[0] = 127.0;
        let events = [
            Event::Start(Start {
                instance: "catalog-instance".into(),
                revision: Some(revision),
                base_revision: None,
                snapshot: true,
                forgets: Vec::new(),
            }),
            Event::Batch(Batch::Conversations {
                records: vec![ConversationRecord {
                    id: "thread".into(),
                    provider: Provider::Codex,
                    title: Some(title.into()),
                    project: Some("/project".into()),
                    repository: Some("repo://project".into()),
                    created_at: Some(1000),
                    updated_at: Some(2000),
                }],
            }),
            Event::Batch(Batch::Messages {
                records: vec![MessageRecord {
                    id: "message".into(),
                    conversation_id: "thread".into(),
                    ordinal: 0,
                    context: crate::message_context::MessageContext::Linear,
                    role: crate::role::Role::User,
                    content: "private body sentinel".into(),
                    projection: "private body sentinel".into(),
                    created_at: Some(1000),
                    generation: Some(self.identity.generation.clone()),
                    vector: Some(vector),
                }],
            }),
            Event::Finish,
        ];
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for event in events {
            writeln!(file, "{}", serde_json::to_string(&event).unwrap()).unwrap();
        }
        self.store.index(&self.identity, file).await
    }

    async fn catalog(&self) -> (Head, BTreeMap<String, ConversationRecord>) {
        let head = self.store.head(&self.identity).await.unwrap().0.unwrap();
        assert!(head.catalog_ready);
        let key = catalog_key(&self.identity.group_id, &head.version);
        let raw: String = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut self.store.connection.clone())
            .await
            .unwrap();
        let manifest: CanonicalManifest = decode(&raw).unwrap();
        let mut bytes = Vec::new();
        for chunk in 0..manifest.chunks {
            let key = format!("{key}:chunk:{chunk}");
            let part: Vec<u8> = redis::cmd("GET")
                .arg(&key)
                .query_async(&mut self.store.connection.clone())
                .await
                .unwrap();
            assert!(part.len() <= CANONICAL_CHUNK_BYTES);
            let ttl: i64 = redis::cmd("TTL")
                .arg(&key)
                .query_async(&mut self.store.connection.clone())
                .await
                .unwrap();
            assert_eq!(ttl, -1, "committed catalog chunks persist");
            bytes.extend(part);
        }
        assert_eq!(bytes.len(), manifest.bytes);
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), manifest.digest);
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("private body sentinel"));
        assert!(!text.contains("\"vector\""));
        (head, decode(&text).unwrap())
    }

    pub(super) async fn cleanup(self) {
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(format!("{}*", prefix(&self.identity.group_id)))
            .query_async(&mut self.store.connection.clone())
            .await
            .unwrap();
        if !keys.is_empty() {
            redis::cmd("UNLINK")
                .arg(keys)
                .query_async::<i64>(&mut self.store.connection.clone())
                .await
                .unwrap();
        }
        redis::cmd("FT.DROPINDEX")
            .arg(index_name(&self.identity.group_id))
            .query_async::<()>(&mut self.store.connection.clone())
            .await
            .unwrap();
    }
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn catalogs_follow_publication_reuse_replacement_and_forgetting() {
    let fixture = Fixture::new().await;
    fixture.upload(1, "first title").await.unwrap();
    let (first, rows) = fixture.catalog().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows["thread"].title.as_deref(), Some("first title"));
    assert_eq!(rows["thread"].provider, Provider::Codex);
    assert_eq!(rows["thread"].updated_at, Some(2000));
    fixture.upload(1, "first title").await.unwrap();
    assert_eq!(fixture.catalog().await.0.version, first.version);

    let manifest_key = format!(
        "{}stage:{}:keys",
        prefix(&fixture.identity.group_id),
        first.version
    );
    let mut retired: Vec<String> = redis::cmd("LRANGE")
        .arg(&manifest_key)
        .arg(0)
        .arg(-1)
        .query_async(&mut fixture.store.connection.clone())
        .await
        .unwrap();
    retired.push(manifest_key);
    fixture.upload(2, "second title").await.unwrap();
    let (second, rows) = fixture.catalog().await;
    assert_ne!(second.version, first.version);
    assert_eq!(rows["thread"].title.as_deref(), Some("second title"));
    for key in retired {
        let exists: bool = redis::cmd("EXISTS")
            .arg(key)
            .query_async(&mut fixture.store.connection.clone())
            .await
            .unwrap();
        assert!(!exists, "every retired stage key is removed");
    }
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
    assert_eq!(fixture.catalog().await.1.len(), 0);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn large_catalogs_are_chunked_and_groups_keep_independent_metadata() {
    let first = Fixture::new().await;
    let second = Fixture::new().await;
    let title = "x".repeat(CANONICAL_CHUNK_BYTES + 1);
    first.upload(1, &title).await.unwrap();
    second.upload(1, "other group title").await.unwrap();
    let (head, rows) = first.catalog().await;
    assert_eq!(rows["thread"].title.as_deref(), Some(title.as_str()));
    let raw: String = redis::cmd("GET")
        .arg(catalog_key(&first.identity.group_id, &head.version))
        .query_async(&mut first.store.connection.clone())
        .await
        .unwrap();
    let manifest: CanonicalManifest = decode(&raw).unwrap();
    assert_eq!(manifest.chunks, 2);
    assert_eq!(
        second.catalog().await.1["thread"].title.as_deref(),
        Some("other group title")
    );
    first.cleanup().await;
    second.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn missing_staged_catalog_prevents_atomic_head_replacement() {
    let fixture = Fixture::new().await;
    fixture.upload(1, "retained title").await.unwrap();
    let (head, expected) = fixture.store.head(&fixture.identity).await.unwrap();
    let mut next = head.unwrap();
    let old_version = next.version.clone();
    next.version = "missing-catalog-stage".into();
    next.revision = 2;
    let missing = catalog_key(&fixture.identity.group_id, &next.version);
    let result = redis::cmd("EVAL")
        .arg(include_str!("../../redis/commit.lua"))
        .arg(2)
        .arg(format!("{}nodes", prefix(&fixture.identity.group_id)))
        .arg(missing)
        .arg(&fixture.identity.node_id)
        .arg(expected)
        .arg(encode(&next).unwrap())
        .arg(1)
        .arg(1)
        .arg("")
        .query_async::<i64>(&mut fixture.store.connection.clone())
        .await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("MASH_STAGE_MISSING")
    );
    let (retained, rows) = fixture.catalog().await;
    assert_eq!(retained.version, old_version);
    assert_eq!(rows["thread"].title.as_deref(), Some("retained title"));
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn explicit_publication_upgrades_old_heads_without_exposing_failed_snapshots() {
    let fixture = Fixture::new().await;
    fixture.upload(1, "retained title").await.unwrap();
    let (mut previous, _) = fixture.catalog().await;
    let old_version = previous.version.clone();
    previous.catalog_ready = false;
    redis::cmd("HSET")
        .arg(format!("{}nodes", prefix(&fixture.identity.group_id)))
        .arg(&fixture.identity.node_id)
        .arg(encode(&previous).unwrap())
        .query_async::<i64>(&mut fixture.store.connection.clone())
        .await
        .unwrap();
    let error = fixture
        .store
        .list_threads(&fixture.identity, crate::thread_catalog::Request::default())
        .await
        .unwrap_err();
    assert!(error.error.message.contains("mash index"));
    fixture.upload(1, "retained title").await.unwrap();
    let (upgraded, _) = fixture.catalog().await;
    assert_ne!(upgraded.version, old_version);
    assert_eq!(upgraded.revision, previous.revision);
    assert_eq!(upgraded.last_updated_at, previous.last_updated_at);
    assert!(fixture.upload(0, "rejected title").await.is_err());
    let (retained, rows) = fixture.catalog().await;
    assert_eq!(retained.version, upgraded.version);
    assert_eq!(rows["thread"].title.as_deref(), Some("retained title"));
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn catalog_reads_need_no_message_payload_and_detect_retired_versions() {
    let fixture = Fixture::new().await;
    fixture.upload(1, "old title").await.unwrap();
    let old = fixture
        .store
        .head(&fixture.identity)
        .await
        .unwrap()
        .0
        .unwrap();
    let heads = [(fixture.identity.node_id.clone(), old)].into();
    fixture.upload(2, "current title").await.unwrap();
    let error = fixture
        .store
        .catalog_page(
            &fixture.identity,
            crate::thread_catalog::Request::default(),
            heads,
        )
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Conflict);

    let current = fixture
        .store
        .head(&fixture.identity)
        .await
        .unwrap()
        .0
        .unwrap();
    redis::cmd("UNLINK")
        .arg(data_key(&fixture.identity.group_id, &current.version))
        .arg(format!(
            "{}:chunk:0",
            data_key(&fixture.identity.group_id, &current.version)
        ))
        .arg(document_key(
            &fixture.identity.group_id,
            &current.version,
            "message",
        ))
        .query_async::<i64>(&mut fixture.store.connection.clone())
        .await
        .unwrap();
    let page = fixture
        .store
        .list_threads(&fixture.identity, crate::thread_catalog::Request::default())
        .await
        .unwrap();
    assert_eq!(page.threads.len(), 1);
    assert_eq!(
        page.threads[0].metadata.title.as_deref(),
        Some("current title")
    );
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn catalog_reads_reject_oversized_manifests_bad_checksums_and_missing_chunks() {
    let fixture = Fixture::new().await;
    fixture.upload(1, "retained title").await.unwrap();
    let head = fixture
        .store
        .head(&fixture.identity)
        .await
        .unwrap()
        .0
        .unwrap();
    let key = catalog_key(&fixture.identity.group_id, &head.version);
    let original: String = redis::cmd("GET")
        .arg(&key)
        .query_async(&mut fixture.store.connection.clone())
        .await
        .unwrap();
    for oversized in [true, false] {
        let mut manifest: CanonicalManifest = decode(&original).unwrap();
        if oversized {
            manifest.bytes = fixture.store.max_node_bytes + 1;
        } else {
            manifest.digest = "bad checksum".into();
        }
        redis::cmd("SET")
            .arg(&key)
            .arg(encode(&manifest).unwrap())
            .query_async::<()>(&mut fixture.store.connection.clone())
            .await
            .unwrap();
        let error = fixture
            .store
            .list_threads(&fixture.identity, crate::thread_catalog::Request::default())
            .await
            .unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
    }
    redis::cmd("SET")
        .arg(&key)
        .arg(original)
        .query_async::<()>(&mut fixture.store.connection.clone())
        .await
        .unwrap();
    redis::cmd("UNLINK")
        .arg(format!("{key}:chunk:0"))
        .query_async::<i64>(&mut fixture.store.connection.clone())
        .await
        .unwrap();
    let error = fixture
        .store
        .list_threads(&fixture.identity, crate::thread_catalog::Request::default())
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
    fixture.cleanup().await;
}
