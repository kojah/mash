//! Disposable local gold with deterministic local inference.
use super::{Local, Path, ResolvedConfig, Storage, build};

pub fn qdrant_configuration() -> crate::config::QdrantConfig {
    crate::config::QdrantConfig {
        url: std::env::var("MASH_TEST_QDRANT_URL").unwrap(),
        grpc_url: std::env::var("MASH_TEST_QDRANT_GRPC_URL").unwrap(),
        ..Default::default()
    }
}

pub async fn cleanup(local: &Local, url: &str) {
    for suffix in ["state", "messages"] {
        reqwest::Client::new()
            .delete(format!(
                "{url}/collections/mash_views_{}_{}",
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

pub fn configure(config: &Path) -> String {
    use std::io::Write;
    let qdrant = qdrant_configuration();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(config)
        .unwrap();
    writeln!(
        file,
        "\n[qdrant]\nurl={:?}\ngrpc_url={:?}",
        qdrant.url, qdrant.grpc_url
    )
    .unwrap();
    qdrant.url
}

/// Build lexical gold for public-interface fixtures using their actual configuration.
pub async fn lexical(database: &Path, config: &Path) -> (Local, String) {
    let url = configure(config);
    let configuration = crate::config::load(Some(config)).unwrap();
    assert!(
        build(
            database,
            &database.parent().unwrap().join("no-models"),
            &configuration,
            false
        )
        .await
        .unwrap()
        .ready
    );
    (Local::open(database, &configuration).await.unwrap(), url)
}

pub struct Fixture {
    pub configuration: ResolvedConfig,
    pub local: Local,
    pub model: crate::semantic::test_embeddings::Fixture,
}

impl Fixture {
    pub async fn new(database: &Path, node: &str) -> Self {
        let root = database.parent().unwrap();
        let path = root.join("gold-test.toml");
        std::fs::write(&path, "version=1\nproviders={}\n").unwrap();
        let mut configuration = crate::config::load(Some(&path)).unwrap();
        configuration.qdrant = qdrant_configuration();
        configuration.shared = Some(crate::config::SharedConfig {
            group_id: "unused".into(),
            node_id: node.into(),
            api_url: "http://127.0.0.1:9".into(),
            token_env: "HOME".into(),
            publication_redaction: crate::redaction::Policy::ON,
        });
        let model = crate::semantic::test_embeddings::Fixture::at(&root.join("no-models"), 384);
        let mut writer = Storage::open_writer(database).unwrap();
        for provider in crate::provider::Provider::ALL {
            writer.begin_silver_scan(*provider).unwrap();
            writer.finish_silver_scan(*provider, true).unwrap();
        }
        writer.finish_silver_ingestion().unwrap();
        writer.commit_writer().unwrap();
        drop(writer);
        let models = root.join("no-models");
        assert!(
            build(database, &models, &configuration, false)
                .await
                .unwrap()
                .ready
        );
        let local = Local::open(database, &configuration).await.unwrap();
        Self {
            configuration,
            local,
            model,
        }
    }

    pub async fn candidates(
        &self,
        query: &str,
        mut vector: Vec<f64>,
        filter: Option<crate::search_filter::Filter>,
    ) -> crate::views::retrieval::Candidates {
        if !vector.is_empty() {
            vector.resize(384, 0.0);
        }
        self.local
            .candidates(crate::views::retrieval::Search {
                match_mode: crate::views::matching::Mode::Strict,
                vector_policy: Some(crate::views::vector::Search::Exact {}),
                intent: crate::views::exact::Intent::RelatedDiscussion,
                unit: crate::search_results::SearchUnit::Messages,
                query: query.into(),
                generation: self.local.scope.generation.clone(),
                vector,
                provider: None,
                days: None,
                exclude_self: false,
                filter,
            })
            .await
            .unwrap()
    }

    pub async fn cleanup(&self) {
        for suffix in ["state", "messages"] {
            reqwest::Client::new()
                .delete(format!(
                    "{}/collections/mash_views_{}_{}",
                    self.configuration.qdrant.url,
                    blake3::hash(self.local.scope.group_id.as_bytes()).to_hex(),
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
