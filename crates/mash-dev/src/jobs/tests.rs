use super::*;
use crate::config;

fn fixture(directory: &std::path::Path, build: Vec<Command>, mcp: Command) -> Config {
    let node = Node::History {
        build: "platform".into(),
        install: vec![Command {
            argv: vec![
                "sh".into(),
                "-c".into(),
                "echo installed >> installed".into(),
            ],
        }],
        mcp,
        shared: false,
        requires: vec![],
    };
    Config {
        repository: directory.into(),
        logs: directory.join("logs"),
        timeout_seconds: 30,
        builds: BTreeMap::from([("platform".into(), build)]),
        nodes: BTreeMap::from([("one".into(), node.clone()), ("two".into(), node)]),
    }
}

async fn finished(runner: &Runner, id: u64) -> Job {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let job = runner.get(id).await.unwrap();
            if job.state != State::Running {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn failed_shared_build_runs_once_and_never_installs() {
    let directory = tempfile::tempdir().unwrap();
    let runner = Runner::new(fixture(
        directory.path(),
        vec![Command {
            argv: vec![
                "sh".into(),
                "-c".into(),
                "echo built >> built; exit 1".into(),
            ],
        }],
        Command {
            argv: vec!["unreachable-mcp".into()],
        },
    ));
    let job = runner
        .start(vec!["one".into(), "two".into()], Operation::Deploy)
        .await
        .unwrap();
    let job = finished(&runner, job.id).await;
    assert!(job.state == State::Failed);
    assert!(job.nodes.values().all(|n| n.state == State::Failed));
    assert_eq!(
        std::fs::read_to_string(directory.path().join("built")).unwrap(),
        "built\n"
    );
    assert!(!directory.path().join("installed").exists());
}

#[tokio::test]
async fn selection_validation_and_job_exclusion_happen_before_commands() {
    let directory = tempfile::tempdir().unwrap();
    let runner = Runner::new(fixture(
        directory.path(),
        vec![Command {
            argv: vec!["sh".into(), "-c".into(), "exit 1".into()],
        }],
        Command {
            argv: vec!["unreachable-mcp".into()],
        },
    ));
    for nodes in [
        vec![],
        vec!["unknown".into()],
        vec!["one".into(), "one".into()],
    ] {
        assert!(runner.start(nodes, Operation::Deploy).await.is_err());
    }
    let job = runner
        .start(vec!["one".into()], Operation::Deploy)
        .await
        .unwrap();
    assert!(
        runner
            .start(vec!["two".into()], Operation::Refresh(RefreshMode::Sync))
            .await
            .is_err()
    );
    finished(&runner, job.id).await;
    assert!(runner.get(u64::MAX).await.is_err());
}

#[tokio::test]
async fn failed_api_blocks_dependent_install_and_refresh_before_mcp_starts() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = fixture(
        directory.path(),
        vec![Command {
            argv: vec!["true".into()],
        }],
        Command {
            argv: vec!["unreachable-mcp".into()],
        },
    );
    config.nodes.insert(
        "api".into(),
        Node::Api {
            build: "platform".into(),
            install: vec![Command {
                argv: vec!["true".into()],
            }],
            verify: vec![Command {
                argv: vec!["false".into()],
            }],
        },
    );
    if let Node::History { requires, .. } = config.nodes.get_mut("one").unwrap() {
        requires.push("api".into());
    }
    config.validate().unwrap();
    let runner = Runner::new(config);
    assert!(
        runner
            .start(vec!["one".into()], Operation::Deploy)
            .await
            .is_err()
    );
    assert!(
        runner
            .start(vec!["one".into(), "api".into()], Operation::Deploy)
            .await
            .is_err()
    );
    assert!(
        runner
            .start(vec!["api".into()], Operation::Refresh(RefreshMode::Sync))
            .await
            .is_err()
    );
    let id = runner
        .start(vec!["api".into(), "one".into()], Operation::Deploy)
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert!(job.state == State::Failed);
    assert!(
        job.nodes["one"]
            .error
            .as_ref()
            .unwrap()
            .contains("required API")
    );
    assert!(!directory.path().join("installed").exists());
    let id = runner
        .start(vec!["one".into()], Operation::Refresh(RefreshMode::Sync))
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert!(
        job.nodes["one"]
            .error
            .as_ref()
            .unwrap()
            .contains("api-check")
    );
    assert!(!directory.path().join("installed").exists());
}

#[test]
fn strict_configuration_rejects_empty_commands_unknown_keys_and_unknown_builds() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = fixture(
        directory.path(),
        vec![Command { argv: vec![] }],
        Command {
            argv: vec!["mash".into(), "mcp".into()],
        },
    );
    assert!(config.validate().is_err());
    config.builds.get_mut("platform").unwrap()[0].argv = vec!["true".into()];
    assert!(config.validate().is_ok());
    if let Node::History { build, .. } = config.nodes.get_mut("one").unwrap() {
        *build = "unknown".into();
    }
    assert!(config.validate().is_err());
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "unknown='value'\n").unwrap();
    assert!(config::load(&path).is_err());
}

#[tokio::test]
#[ignore = "requires MASH_TEST_SERVER_BINARY pointing at a built Mash"]
async fn real_mash_refresh_progress_rebuild_and_deploy_without_ingestion() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let binary = std::env::var("MASH_TEST_SERVER_BINARY").unwrap();
    let config = root.join("node.toml");
    let database = root.join("node.sqlite3");
    std::fs::write(&config, "version=1\nproviders={}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n").unwrap();
    let runner = Runner::new(fixture(
        root,
        vec![Command {
            argv: vec!["sh".into(), "-c".into(), "echo built >> built".into()],
        }],
        Command {
            argv: vec![
                binary,
                "--config".into(),
                config.to_str().unwrap().into(),
                "--db".into(),
                database.to_str().unwrap().into(),
                "--models-dir".into(),
                root.join("missing-models").to_str().unwrap().into(),
                "mcp".into(),
            ],
        },
    ));
    let id = runner
        .start(vec!["one".into()], Operation::Refresh(RefreshMode::Sync))
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert!(
        job.state == State::Succeeded,
        "{}",
        serde_json::to_string(&job).unwrap()
    );
    assert!(database.exists());
    assert_eq!(
        job.nodes["one"].results[1]
            .structured_content
            .as_ref()
            .unwrap()["ready"],
        true
    );
    assert!(job.nodes["one"].progress.is_some());
    assert!(!root.join("built").exists());
    assert_eq!(
        job.nodes["one"].results[0]
            .structured_content
            .as_ref()
            .unwrap()["rebuild_local_views"],
        false
    );

    let id = runner
        .start(
            vec!["one".into()],
            Operation::Refresh(RefreshMode::LocalViews),
        )
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert!(job.state == State::Succeeded);
    assert_eq!(
        job.nodes["one"].results[0]
            .structured_content
            .as_ref()
            .unwrap()["rebuild_local_views"],
        true
    );
    assert!(!root.join("missing-models").exists());
    let before = std::fs::read(&database).unwrap();
    let id = runner
        .start(vec!["one".into(), "two".into()], Operation::Deploy)
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert!(job.state == State::Succeeded);
    assert!(job.nodes.values().all(|n| n.results.len() == 1));
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(root.join("built")).unwrap(),
        "built\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("installed")).unwrap(),
        "installed\ninstalled\n"
    );
}
