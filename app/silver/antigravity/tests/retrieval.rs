use super::*;
use crate::core::{IndexRequest, Operation, Paths, Request, Response};

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn core_searches_native_antigravity_messages_and_threads_with_local_models() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path());
    let config = dir.path().join("config.toml");
    std::fs::write(&config,toml::to_string(&toml::toml!{version=1 providers={antigravity={roots=[(source.to_str().unwrap())]}} index={all_history=true}}).unwrap()).unwrap();
    let paths = Paths {
        config: Some(config),
        db: Some(dir.path().join("index.sqlite3")),
        models_dir: Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into()),
    };
    crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .unwrap();
    for unit in [
        crate::search_results::SearchUnit::Messages,
        crate::search_results::SearchUnit::Threads,
    ] {
        let Response::Search(result) = crate::core::execute(Request {
            paths: paths.clone(),
            operation: Operation::Search {
                fusion: crate::gold::config::FusionOverrides::default(),
                ranking: None,
                diagnostics: false,
                query: "cbioportal study filters".into(),
                intent: None,
                limit: Some(5),
                unit: Some(unit),
                filters: Box::new(crate::search_filter::Selection {
                    provider: vec![Provider::Antigravity],
                    ..Default::default()
                }),
                shared: false,
                local: true,
            },
        })
        .await
        .unwrap() else {
            panic!("search result")
        };
        match result.results {
            crate::search_results::Results::Messages(messages) => {
                assert!(!messages.is_empty());
                assert!(messages.iter().all(|m| m.provider == Provider::Antigravity));
            }
            crate::search_results::Results::Threads(threads) => {
                assert_eq!(threads.len(), 1);
                assert_eq!(threads[0].provider, Provider::Antigravity);
            }
        }
    }
}
