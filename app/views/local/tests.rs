//! Direct-local lifecycle acceptance against a disposable Qdrant instance.
use super::*;
use crate::silver::{Chat, NormalizedMessage, SourceIdentity};
use std::path::PathBuf;

struct Fixture {
    directory: tempfile::TempDir,
    database: PathBuf,
    configuration: ResolvedConfig,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        std::fs::write(&config, "version = 1\n[providers]\n").unwrap();
        let mut configuration = crate::config::load(Some(&config)).unwrap();
        configuration.qdrant.url = std::env::var("MASH_TEST_QDRANT_URL").unwrap();
        configuration.qdrant.grpc_url = std::env::var("MASH_TEST_QDRANT_GRPC_URL").unwrap();
        configuration.views.build.representations =
            crate::views::config::Representations::LexicalOnly;
        let database = directory.path().join("silver.sqlite3");
        Self {
            directory,
            database,
            configuration,
        }
    }

    fn write(&self, projection: &str) {
        let mut storage = Storage::open_writer(&self.database).unwrap();
        storage
            .begin_silver_scan(crate::provider::Provider::Codex)
            .unwrap();
        storage
            .replace_chat(&Chat {
                id: "thread".into(),
                provider: crate::provider::Provider::Codex,
                title_origin: None,
                timestamp_origins: None,
                claude_lineage: None,
                source: SourceIdentity::file(self.directory.path().join("provider.jsonl")),
                project: None,
                repository: None,
                title: Some("fixture".into()),
                created_at: Some(1),
                updated_at: Some(2),
                messages: vec![NormalizedMessage {
                    attachments: Vec::new(),
                    id: "message".into(),
                    ordinal: 0,
                    context: crate::message_context::MessageContext::Linear,
                    role: crate::role::Role::User,
                    content: "canonical private context".into(),
                    dialogue_text: Some(projection.into()),
                    created_at: Some(2),
                    tools: crate::tools::Activity::default(),
                }]
                .into(),
            })
            .unwrap();
        storage
            .finish_silver_scan(crate::provider::Provider::Codex, true)
            .unwrap();
        storage.finish_silver_ingestion().unwrap();
        storage.commit_writer().unwrap();
    }

    async fn build(&self, full: bool) -> Build {
        let owner = crate::storage::admission::acquire(&self.database).unwrap();
        let result = crate::storage::admission::scope(
            Some(owner),
            build(
                &self.database,
                &self.directory.path().join("no-models"),
                &self.configuration,
                full,
            ),
        )
        .await
        .unwrap();
        assert!(result.ready);
        assert_eq!(result.embeddings.model_inferences, 0);
        result
    }

    async fn open(&self) -> Local {
        Local::open(&self.database, &self.configuration)
            .await
            .unwrap()
    }

    async fn cleanup(&self, local: &Local) {
        for suffix in ["state", "messages"] {
            reqwest::Client::new()
                .delete(format!(
                    "{}/collections/mash_views_{}_{}",
                    self.configuration.qdrant.url,
                    blake3::hash(local.scope.group_id.as_bytes()).to_hex(),
                    suffix,
                ))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        }
    }
}

fn query(text: &str) -> crate::views::retrieval::Search {
    crate::views::retrieval::Search {
        match_mode: crate::views::matching::Mode::Strict,
        vector_policy: None,
        intent: crate::views::exact::Intent::ExactMention,
        unit: crate::search_results::SearchUnit::default(),
        query: text.into(),
        generation: semantic::SHARED_EMBEDDING_GENERATION.into(),
        vector: Vec::new(),
        provider: None,
        days: None,
        exclude_self: false,
        filter: None,
    }
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
async fn local_qdrant_exact_uses_certified_recipe_without_activating_dense_changes() {
    let mut fixture = Fixture::new();
    fixture.write("native sentinel");
    fixture.build(false).await;
    let original = fixture.open().await;
    let version = original
        .store
        .views_configuration(&original.scope)
        .await
        .unwrap()
        .version;
    fixture.configuration.views.build.embedding_text =
        crate::views::config::EmbeddingText::TitleAndDialogue;
    assert!(
        Local::open(&fixture.database, &fixture.configuration)
            .await
            .is_err()
    );
    let exact = Local::open_exact(&fixture.database, &fixture.configuration)
        .await
        .unwrap();
    assert_eq!(
        exact
            .candidates(query("sentinel"))
            .await
            .unwrap()
            .lexical
            .len(),
        1
    );
    assert_eq!(
        original
            .store
            .views_configuration(&original.scope)
            .await
            .unwrap()
            .version,
        version
    );
    fixture.write("changed history");
    assert!(exact.candidates(query("sentinel")).await.is_err());
    fixture.cleanup(&original).await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
async fn local_qdrant_reader_rejects_replaced_silver_with_the_same_certificate() {
    let fixture = Fixture::new();
    fixture.write("native sentinel");
    fixture.build(false).await;
    let original = fixture.open().await;
    assert!(original.current().await.unwrap().is_some());
    // A replacement database may have the same revision and completeness.
    let replacement = Fixture::new();
    replacement.write("replacement history");
    let storage = Storage::open_silver(&replacement.database).unwrap();
    assert_eq!(storage.silver_certificate().unwrap(), original.certificate);
    drop(storage);
    std::fs::copy(&replacement.database, &fixture.database).unwrap();
    assert!(original.current().await.unwrap().is_none());
    assert!(original.candidates(query("sentinel")).await.is_err());
    fixture.cleanup(&original).await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
async fn local_qdrant_failed_dense_rebuild_preserves_silver_without_certifying_gold() {
    let mut fixture = Fixture::new();
    fixture.write("retained sentinel");
    fixture.build(false).await;
    let local = fixture.open().await;
    let before = Storage::open_silver(&fixture.database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    fixture.configuration.views.build.representations =
        crate::views::config::Representations::LexicalAndDense;
    let error = build(
        &fixture.database,
        &fixture.directory.path().join("missing-models"),
        &fixture.configuration,
        true,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Model);
    assert_eq!(
        Storage::open_silver(&fixture.database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        before
    );
    assert!(fixture.open().await.current().await.unwrap().is_none());
    fixture.configuration.views.build.representations =
        crate::views::config::Representations::LexicalOnly;
    fixture.build(true).await;
    assert_eq!(
        fixture
            .open()
            .await
            .candidates(query("retained"))
            .await
            .unwrap()
            .lexical
            .len(),
        1
    );
    fixture.cleanup(&local).await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
async fn local_qdrant_replay_preserves_silver_and_rejects_stale_or_hidden_results() {
    let fixture = Fixture::new();
    fixture.write("idempotency redis4");
    let before = Storage::open_silver(&fixture.database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    fixture.build(false).await;
    let local = fixture.open().await;
    assert_eq!(
        local.scope.generation,
        semantic::SHARED_EMBEDDING_GENERATION
    );
    let candidates = local.candidates(query("idempotency")).await.unwrap();
    assert_eq!(candidates.lexical.len(), 1);
    assert!(
        candidates.lexical[0]
            .turn_id
            .as_ref()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(
        local
            .candidates(query("private"))
            .await
            .unwrap()
            .lexical
            .is_empty()
    );
    assert_eq!(
        Storage::open_silver(&fixture.database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        before
    );
    fixture.build(true).await;
    assert_eq!(
        Storage::open_silver(&fixture.database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        before
    );

    fixture.write("");
    assert!(local.current().await.unwrap().is_none());
    assert!(local.candidates(query("idempotency")).await.is_err());
    fixture.build(false).await;
    let hidden = fixture.open().await;
    assert!(
        hidden
            .candidates(query("idempotency"))
            .await
            .unwrap()
            .lexical
            .is_empty()
    );
    assert_eq!(
        Storage::open_silver(&fixture.database)
            .unwrap()
            .view("message", 0)
            .unwrap()[0]
            .content,
        "canonical private context"
    );

    fixture.write("replacement sentinel");
    fixture.build(false).await;
    let current = fixture.open().await;
    assert_eq!(
        current
            .candidates(query("replacement"))
            .await
            .unwrap()
            .lexical
            .len(),
        1
    );
    Storage::open_existing(&fixture.database)
        .unwrap()
        .forget("thread")
        .unwrap();
    assert!(current.current().await.unwrap().is_none());
    fixture.build(false).await;
    assert!(
        fixture
            .open()
            .await
            .candidates(query("replacement"))
            .await
            .unwrap()
            .lexical
            .is_empty()
    );
    fixture.cleanup(&current).await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
async fn local_qdrant_copied_silver_has_no_certificate_in_the_original_namespace() {
    let fixture = Fixture::new();
    fixture.write("original sentinel");
    fixture.build(false).await;
    let original = fixture.open().await;
    Storage::open_existing(&fixture.database)
        .unwrap()
        .truncate_wal()
        .unwrap();
    let copy = fixture.directory.path().join("copy.sqlite3");
    std::fs::copy(&fixture.database, &copy).unwrap();
    // Opening a copied database must not certify it from the original's collections.
    match Local::open(&copy, &fixture.configuration).await {
        Ok(copied) => {
            assert_ne!(copied.scope.group_id, original.scope.group_id);
            assert!(copied.current().await.unwrap().is_none());
        }
        Err(error) => assert_ne!(error.error.message, ""),
    }
    assert!(original.current().await.unwrap().is_some());
    fixture.cleanup(&original).await;
}
