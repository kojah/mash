//! Same natural-language judgments through the full shared hybrid-search path.
use super::*;
use crate::core::{self, IndexRequest, Operation, Request};
use crate::semantic::evaluation;

async fn contract() {
    let token = std::env::var("PATH").unwrap();
    let service = Service::with_parameters(true, 384, 64 * 1024 * 1024, &token).await;
    let (_root, paths) = evaluation::fixture();
    core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .unwrap();
    let config_path = paths.config.as_deref().unwrap();
    let local_config = crate::config::load(Some(config_path)).unwrap();
    let local = crate::views::local::Local::open(paths.db.as_deref().unwrap(), &local_config)
        .await
        .unwrap();
    let mut document: toml::Table =
        toml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    document.insert(
        "shared".into(),
        toml::Value::Table(toml::toml! {
            group_id = (service.group.clone())
            node_id = "first"
            api_url = (service.url.clone())
            token_env = "PATH"
        }),
    );
    std::fs::write(config_path, toml::to_string(&document).unwrap()).unwrap();
    let config = crate::config::load(Some(config_path))
        .unwrap()
        .shared
        .unwrap();
    assert!(
        !crate::shared::publish(
            paths.db.as_deref().unwrap(),
            &config,
            Some(crate::shared::Inference {
                models_dir: paths.models_dir.as_deref().unwrap(),
                embeddings: local_config.embeddings.as_ref(),
            })
        )
        .await
        .is_pending()
    );
    let failures = evaluation::evaluate(&paths, true).await;
    service.cleanup().await;
    crate::views::local::test_support::cleanup(&local, &local_config.qdrant.url).await;
    assert_eq!(
        failures,
        Vec::<String>::new(),
        "shared relevance regressions"
    );
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL and installed MASH_TEST_MODELS_DIR"]
async fn qdrant_labeled_hybrid_relevance_matrix_uses_real_models_and_core() {
    contract().await;
}
