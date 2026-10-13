use super::*;
use crate::mcp::watching::shutdown;
use std::time::Duration;

fn fixture(shared: bool) -> (tempfile::TempDir, Mash) {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let routing = if shared {
        "[index]\ndestination='both'\n[shared]\ngroup_id='watch-test'\nnode_id='test'\napi_url='http://127.0.0.1:1'\ntoken_env='MASH_WATCH_TEST_UNSET_TOKEN'\n"
    } else {
        ""
    };
    std::fs::write(
        &config,
        format!(
            "version=1\nproviders={{}}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n{routing}"
        ),
    )
    .unwrap();
    let server = Mash {
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(directory.path().join("index.sqlite3")),
            models_dir: Some(directory.path().join("models")),
        }),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
        watcher: Arc::default(),
    };
    (directory, server)
}

async fn until(server: &Mash, predicate: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    tokio::time::timeout(crate::test_support::HANG_GUARD, async {
        loop {
            let result = server.watch_status().await;
            assert!(
                result.content[0]
                    .as_text()
                    .unwrap()
                    .text
                    .starts_with("Mash watcher")
            );
            let value = result.structured_content.unwrap();
            if predicate(&value) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("watcher state transition")
}

#[tokio::test]
async fn watcher_lifecycle_is_idempotent_and_stop_does_not_abort_a_pass() {
    let (_directory, server) = fixture(false);
    assert_eq!(
        server.watch_status().await.structured_content.unwrap()["state"],
        "stopped"
    );
    let gate = server.gate.lock().await;
    let start = server
        .watch_start(Parameters(WatchStart {
            debounce_ms: Some(50),
        }))
        .await;
    assert_ne!(start.is_error, Some(true));
    assert!(
        start.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("Mash watcher — \"running\"")
    );
    assert!(
        start.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("Debounce (ms): 50")
    );
    let duplicate = server
        .watch_start(Parameters(WatchStart { debounce_ms: None }))
        .await;
    assert_eq!(duplicate.structured_content.unwrap()["debounce_ms"], 50);
    until(&server, |v| {
        v["indexing"] == true && v["progress"]["phase"] == "waiting-for-index"
    })
    .await;
    assert_eq!(
        server.watch_stop().await.structured_content.unwrap()["state"],
        "stopping"
    );
    assert_eq!(
        server.watch_stop().await.structured_content.unwrap()["state"],
        "stopping"
    );
    assert_eq!(
        server
            .watch_start(Parameters(WatchStart { debounce_ms: None }))
            .await
            .is_error,
        Some(true)
    );
    drop(gate);
    let stopped = until(&server, |v| v["state"] == "stopped").await;
    assert_eq!(stopped["indexing"], false);
    assert!(stopped["last_error"].is_null());
    assert!(stopped["last_refresh_at"].is_number());
    assert!(stopped["last_result"]["silver"].is_object());
    let restart = server
        .watch_start(Parameters(WatchStart {
            debounce_ms: Some(100),
        }))
        .await;
    assert_eq!(restart.structured_content.unwrap()["debounce_ms"], 100);
    shutdown(&server).await;
    assert_eq!(
        server.watch_status().await.structured_content.unwrap()["state"],
        "stopped"
    );
}

#[tokio::test]
async fn watcher_reports_configuration_errors_and_rejects_invalid_debounce() {
    let (_directory, server) = fixture(false);
    for value in [0, 49, 60_001] {
        assert_eq!(
            server
                .watch_start(Parameters(WatchStart {
                    debounce_ms: Some(value)
                }))
                .await
                .is_error,
            Some(true)
        );
    }
    assert_eq!(
        server.watch_status().await.structured_content.unwrap()["state"],
        "stopped"
    );
    std::fs::write(server.paths.config.as_ref().unwrap(), "invalid TOML").unwrap();
    server
        .watch_start(Parameters(WatchStart { debounce_ms: None }))
        .await;
    let stopped = until(&server, |v| {
        v["state"] == "stopped" && v["last_error"].is_object()
    })
    .await;
    assert_eq!(stopped["last_error"]["error"]["kind"], "configuration");
    let result = server.watch_status().await;
    let readable = &result.content[0].as_text().unwrap().text;
    assert!(readable.contains("Last indexing error:\nMash error"));
    assert!(readable.contains("Retryable:"));
    assert!(!server.paths.db.as_ref().unwrap().exists());
    shutdown(&server).await;
}

#[tokio::test]
async fn watcher_status_retains_pending_shared_publication() {
    let (_directory, server) = fixture(true);
    server
        .watch_start(Parameters(WatchStart {
            debounce_ms: Some(50),
        }))
        .await;
    let value = until(&server, |v| {
        v["last_result"]["shared"]["state"] == "pending"
    })
    .await;
    assert!(value["last_result"]["shared"]["error"].is_string());
    assert!(value["last_result"]["silver"].is_object());
    shutdown(&server).await;
}

#[tokio::test]
async fn sessions_sharing_a_database_elect_one_leader_and_report_roles() {
    let (_directory, leader) = fixture(false);
    let standby = Mash {
        paths: leader.paths.clone(),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
        watcher: Arc::default(),
    };
    assert!(
        leader.watch_status().await.structured_content.unwrap()["role"].is_null(),
        "a stopped watcher has no role"
    );
    leader
        .watch_start(Parameters(WatchStart {
            debounce_ms: Some(50),
        }))
        .await;
    until(&leader, |v| {
        v["role"] == "leader" && v["last_refresh_at"].is_number()
    })
    .await;
    standby
        .watch_start(Parameters(WatchStart {
            debounce_ms: Some(100),
        }))
        .await;
    let waiting = until(&standby, |v| v["role"] == "standby").await;
    assert_eq!(waiting["state"], "running");
    assert_eq!(waiting["indexing"], false);
    assert!(waiting["last_refresh_at"].is_null());
    let text = standby.watch_status().await.content[0]
        .as_text()
        .unwrap()
        .text
        .clone();
    assert!(text.contains("Role: standby"));

    // Stopping a standby only withdraws it from the election.
    standby.watch_stop().await;
    let stopped = until(&standby, |v| v["state"] == "stopped").await;
    assert!(stopped["role"].is_null());
    assert!(stopped["last_refresh_at"].is_null());
    standby
        .watch_start(Parameters(WatchStart {
            debounce_ms: Some(100),
        }))
        .await;
    until(&standby, |v| v["role"] == "standby").await;

    // Stopping the leader releases leadership; the standby takes over with a pass.
    leader.watch_stop().await;
    let released = until(&leader, |v| v["state"] == "stopped").await;
    assert!(released["role"].is_null());
    let promoted = until(&standby, |v| {
        v["role"] == "leader" && v["last_refresh_at"].is_number()
    })
    .await;
    assert!(promoted["last_result"]["silver"].is_object());
    assert_eq!(promoted["debounce_ms"], 100);
    shutdown(&standby).await;
    shutdown(&leader).await;
}
