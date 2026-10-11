use super::*;
use crate::config::{Command, Playbooks};

pub(crate) fn fixture(directory: &Path, script: &str) -> Config {
    std::fs::write(directory.join("inventory.yml"), "all: {}\n").unwrap();
    std::fs::write(directory.join("playbook.yml"), "[]\n").unwrap();
    std::fs::write(directory.join("runner.sh"), script).unwrap();
    Config {
        repository: directory.into(),
        logs: directory.join("logs"),
        inventory: directory.join("inventory.yml"),
        timeout_seconds: 30,
        runner: Command {
            argv: vec![
                "sh".into(),
                directory.join("runner.sh").to_str().unwrap().into(),
            ],
        },
        nodes: vec!["one".into(), "two".into()],
        playbooks: Playbooks {
            deploy: "playbook.yml".into(),
            refresh: "playbook.yml".into(),
            check: "playbook.yml".into(),
        },
    }
}

async fn finished(runner: &Runner, id: u64) -> Job {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let job = runner.get(id).await.unwrap();
            if job.state != State::Running {
                return job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn thin_wrapper_delegates_to_runner_and_exposes_only_safe_reports() {
    let directory = tempfile::tempdir().unwrap();
    let runner = Runner::new(fixture(
        directory.path(),
        r#"set -eu
directory=$2
printf '%s\n' "$@" > args
mkdir -p "$directory/artifacts/job/job_events"
echo '{"counter":1,"event_data":{"host":"one","task":"Verify installed MCP","stdout":"PRIVATE_RAW_LOG"}}' > "$directory/artifacts/job/job_events/1.json"
echo '{"state":"succeeded","results":[]}' > "$directory/one.json"
echo PRIVATE_RAW_LOG
"#,
    ));
    let id = runner
        .start(vec!["one".into()], Operation::Deploy)
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert_eq!(job.state, State::Succeeded);
    assert_eq!(
        job.nodes["one"].task.as_deref(),
        Some("Verify installed MCP")
    );
    assert!(
        !serde_json::to_string(&job)
            .unwrap()
            .contains("PRIVATE_RAW_LOG")
    );
    let variables: serde_json::Value =
        serde_json::from_slice(&std::fs::read(job.logs_directory.join("env/extravars")).unwrap())
            .unwrap();
    assert_eq!(variables["mash_selected_nodes"], serde_json::json!(["one"]));
    assert_eq!(variables["mash_operation"], "deploy");
    assert!(
        std::fs::read_to_string(directory.path().join("args"))
            .unwrap()
            .contains("--playbook")
    );
    assert!(
        !directory.path().join("installed").exists(),
        "Rust must not execute fleet installation logic"
    );
}

#[tokio::test]
async fn selection_validation_and_job_exclusion_happen_before_commands() {
    let directory = tempfile::tempdir().unwrap();
    let runner = Runner::new(fixture(directory.path(), "sleep 0.2\nexit 1\n"));
    for nodes in [
        vec![],
        vec!["unknown".into()],
        vec!["one".into(), "one".into()],
    ] {
        assert!(runner.start(nodes, Operation::Deploy).await.is_err());
    }
    let id = runner
        .start(vec!["one".into()], Operation::Deploy)
        .await
        .unwrap()
        .id;
    assert!(
        runner
            .start(vec!["two".into()], Operation::Check)
            .await
            .is_err()
    );
    assert_eq!(finished(&runner, id).await.state, State::Failed);
    assert!(runner.get(u64::MAX).await.is_err());
}

#[tokio::test]
async fn missing_or_corrupt_success_reports_fail_closed_and_offline_skips_are_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let runner = Runner::new(fixture(directory.path(), "exit 0\n"));
    let id = runner
        .start(vec!["one".into()], Operation::Check)
        .await
        .unwrap()
        .id;
    assert_eq!(finished(&runner, id).await.state, State::Failed);
    let runner = Runner::new(fixture(
        directory.path(),
        r#"echo '{"state":"skipped","error":"offline","results":[]}' > "$2/one.json"
"#,
    ));
    let id = runner
        .start(vec!["one".into()], Operation::Check)
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert_eq!(job.state, State::Succeeded);
    assert_eq!(job.nodes["one"].report.state, State::Skipped);
}

#[test]
fn strict_configuration_rejects_legacy_commands_unknown_keys_and_external_playbooks() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = fixture(directory.path(), "");
    assert!(config.validate().is_ok());
    config.playbooks.deploy = "../outside.yml".into();
    assert!(config.validate().is_err());
    config.playbooks.deploy = "playbook.yml".into();
    config.runner.argv.clear();
    assert!(config.validate().is_err());
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "builds={}\n").unwrap();
    assert!(crate::config::load(&path).is_err());
}

#[tokio::test]
#[ignore = "requires built Mash/probe and the locked Ansible runtime"]
async fn real_mash_refresh_progress_rebuild_and_deploy_without_ingestion() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let repository = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .canonicalize()
        .unwrap();
    let binary = std::env::var("MASH_TEST_SERVER_BINARY").unwrap();
    let probe = std::env::var("MASH_TEST_DEV_PROBE_BINARY").unwrap();
    let config = root.join("node.toml");
    let database = root.join("node.sqlite3");
    std::fs::write(&config, "version=1\nproviders={}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n").unwrap();
    let host = serde_json::json!({
        "mash_platform":"fixture", "mash_shared":false,
        "mash_install":[["sh","-c","echo installed >> \"$1\"","fixture",root.join("installed")]],
        "mash_mcp":[binary,"--config",config,"--db",database,"--models-dir",root.join("missing-models"),"mcp"]
    });
    let inventory = root.join("inventory.yml");
    std::fs::write(&inventory, serde_json::to_vec(&serde_json::json!({
        "all":{"vars":{"ansible_connection":"local","ansible_python_interpreter":"/usr/bin/python3","mash_probe":probe,"mash_builds":{"fixture":[["sh","-c","echo built >> \"$1\"","fixture",root.join("built")]]}},"children":{"mash_history":{"hosts":{"one":host,"two":host}}}}
    })).unwrap()).unwrap();
    let runner = Runner::new(Config {
        repository: repository.clone(),
        logs: root.join("jobs"),
        inventory,
        runner: Command {
            argv: vec![
                "uv".into(),
                "run".into(),
                "--project".into(),
                repository.join("deployment").to_str().unwrap().into(),
                "--locked".into(),
                "--no-sync".into(),
                "ansible-runner".into(),
            ],
        },
        nodes: vec!["one".into(), "two".into()],
        timeout_seconds: 60,
        playbooks: Playbooks {
            deploy: "deployment/playbooks/deploy.yml".into(),
            refresh: "deployment/playbooks/refresh.yml".into(),
            check: "deployment/playbooks/check.yml".into(),
        },
    });
    let id = runner
        .start(vec!["one".into()], Operation::Refresh(RefreshMode::Sync))
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert_eq!(
        job.state,
        State::Succeeded,
        "{}",
        serde_json::to_string(&job).unwrap()
    );
    assert!(database.exists());
    assert!(job.nodes["one"].report.progress.is_some());
    assert!(!root.join("built").exists());
    let id = runner
        .start(
            vec!["one".into()],
            Operation::Refresh(RefreshMode::LocalViews),
        )
        .await
        .unwrap()
        .id;
    assert_eq!(finished(&runner, id).await.state, State::Succeeded);
    let before = std::fs::read(&database).unwrap();
    let id = runner
        .start(vec!["one".into(), "two".into()], Operation::Deploy)
        .await
        .unwrap()
        .id;
    let job = finished(&runner, id).await;
    assert_eq!(
        job.state,
        State::Succeeded,
        "{}",
        serde_json::to_string(&job).unwrap()
    );
    assert!(
        job.nodes
            .values()
            .all(|node| node.report.results.len() == 3)
    );
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(root.join("built")).unwrap(),
        "built\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("installed")).unwrap(),
        "installed\ninstalled\n"
    );
    assert!(!root.join("missing-models").exists());
}
