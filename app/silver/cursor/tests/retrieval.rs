use super::*;

#[tokio::test]
#[ignore = "requires disposable Qdrant and explicitly installed models"]
async fn core_retrieves_native_cursor_threads_with_archived_requests_and_project_filters() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    for format in [Format::Agent, Format::Ide] {
        let dir = tempfile::tempdir().unwrap();
        let (path, _connection) = match format {
            Format::Agent => {
                let path = path(dir.path());
                let connection = fixture(&path);
                (path, connection)
            }
            Format::Ide => {
                let path = dir.path().join("User/globalStorage/state.vscdb");
                let connection = super::super::ide::tests::fixture(&path);
                (path, connection)
            }
            Format::WorkspaceState => unreachable!("holds no chats to retrieve"),
        };
        assert!(path.exists());
        let config = dir.path().join("config.toml");
        std::fs::write(&config,toml::to_string(&toml::toml!{version=1 providers={cursor={roots=[(dir.path().to_str().unwrap())]}} index={all_history=true}}).unwrap()).unwrap();
        let url = crate::views::local::test_support::configure(&config);
        let paths = Paths {
            config: Some(config.clone()),
            db: Some(dir.path().join("index.sqlite3")),
            models_dir: Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into()),
        };
        crate::core::execute(Request {
            paths: paths.clone(),
            operation: Operation::Index(IndexRequest::default()),
        })
        .await
        .unwrap();
        for query in ["cbioportal importer", "structural variants"] {
            for unit in [
                crate::search_results::SearchUnit::Chats,
                crate::search_results::SearchUnit::Messages,
            ] {
                let Response::Search(response) = crate::core::execute(Request {
                    paths: paths.clone(),
                    operation: Operation::Search {
                        shared_vector_search: None,
                        fusion: crate::views::config::FusionOverrides::default(),
                        ranking: None,
                        diagnostics: false,
                        query: query.into(),
                        intent: None,
                        limit: Some(5),
                        offset: 0,
                        unit: Some(unit),
                        filters: Box::new(crate::search_filter::Selection {
                            provider: vec![Provider::Cursor],
                            project: vec!["/work/cbioportal".into()],
                            repository: if format == Format::Ide {
                                vec!["/work/cbioportal".into()]
                            } else {
                                Vec::new()
                            },
                            ..Default::default()
                        }),
                        shared: false,
                        local: true,
                    },
                })
                .await
                .unwrap() else {
                    panic!("search")
                };
                match response.results {
                    crate::search_results::Results::Chats(threads) => {
                        assert_eq!(threads.len(), 1);
                        assert_eq!(threads[0].provider, Provider::Cursor);
                    }
                    crate::search_results::Results::Messages(messages) => {
                        assert!(!messages.is_empty());
                        assert!(messages.iter().all(|m| m.provider == Provider::Cursor));
                    }
                }
            }
        }
        let configuration = crate::config::load(Some(&config)).unwrap();
        let local = crate::views::local::Local::open(paths.db.as_ref().unwrap(), &configuration)
            .await
            .unwrap();
        crate::views::local::test_support::cleanup(&local, &url).await;
    }
}
