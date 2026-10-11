//! Upgrade a real executable while one SDK connection remains open.
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde_json::Value;
use std::{path::Path, time::Duration};

async fn help(peer: &rmcp::service::Peer<rmcp::RoleClient>) -> Value {
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        peer.call_tool(CallToolRequestParams::new("help")),
    )
    .await
    .unwrap()
    .unwrap();
    assert_ne!(result.is_error, Some(true));
    result.structured_content.unwrap()
}

fn replace(binary: &Path, source: &Path) {
    let incoming = binary.with_extension("incoming");
    std::fs::copy(source, &incoming).unwrap();
    std::fs::rename(incoming, binary).unwrap();
}

#[tokio::test]
async fn mcp_session_replaces_real_worker_without_reconnecting_or_indexing() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("mash");
    let source = assert_cmd::cargo::cargo_bin!("mash");
    replace(&binary, source);
    let database = directory.path().join("uncreated.sqlite3");
    let models = directory.path().join("uncreated-models");
    let config = directory.path().join("uncreated.toml");
    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("--config")
        .arg(&config)
        .arg("--db")
        .arg(&database)
        .arg("--models-dir")
        .arg(&models)
        .arg("mcp");
    let transport = TokioChildProcess::new(command).unwrap();
    let parent = transport.id().unwrap();
    let client = ().serve(transport).await.unwrap();
    let old = help(&client).await;
    assert_eq!(old["session"]["front_process_id"], parent);
    assert_eq!(old["session"]["generation"], 1);
    let old_worker = old["session"]["worker_process_id"].clone();
    assert_ne!(old_worker, parent);
    replace(&binary, source);
    let new = help(&client).await;
    assert_eq!(new["session"]["front_process_id"], parent);
    assert_eq!(new["session"]["generation"], 2);
    assert_ne!(new["session"]["worker_process_id"], old_worker);
    assert_eq!(new["session"]["state"], "current");
    assert_eq!(new["runtime"]["executable_state"], "current");
    assert_eq!(new["runtime"]["reconnect_required"], false);
    let invalid = client
        .call_tool(
            CallToolRequestParams::new("search").with_arguments(
                serde_json::json!({"query":"test","conversation":["id"]})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(invalid.is_error, Some(true));
    assert!(invalid.content.iter().any(|block| {
        block
            .as_text()
            .is_some_and(|text| text.text.contains("conversation"))
    }));
    assert_eq!(help(&client).await["session"]["generation"], 2);
    assert!(!database.exists());
    assert!(!models.exists());
    assert!(!config.exists());
    client.cancel().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn mcp_session_recovers_worker_crash_immediately_after_a_successful_handover() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("mash");
    let source = assert_cmd::cargo::cargo_bin!("mash");
    replace(&binary, source);
    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("--config")
        .arg(directory.path().join("missing.toml"))
        .arg("mcp");
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    help(&client).await;
    replace(&binary, source);
    let old = help(&client).await;
    assert_eq!(old["session"]["generation"], 2);
    let worker = old["session"]["worker_process_id"].as_u64().unwrap();
    assert!(
        std::process::Command::new("kill")
            .args(["-KILL", &worker.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let recovered = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let result = client
                .call_tool(CallToolRequestParams::new("help"))
                .await
                .unwrap();
            if result.is_error != Some(true) {
                let data = result.structured_content.unwrap();
                if data["session"]["generation"] == 3 {
                    break data;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_ne!(recovered["session"]["worker_process_id"], worker);
    assert_eq!(
        recovered["session"]["front_process_id"],
        old["session"]["front_process_id"]
    );
    assert_eq!(recovered["session"]["state"], "current");
    client.cancel().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn mcp_session_keeps_old_worker_after_failed_update_then_recovers() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("mash");
    let source = assert_cmd::cargo::cargo_bin!("mash");
    replace(&binary, source);
    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("--config")
        .arg(directory.path().join("missing.toml"))
        .arg("mcp");
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let old = help(&client).await;
    let broken = directory.path().join("broken");
    std::fs::write(&broken, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o755)).unwrap();
    replace(&binary, &broken);
    let rejected = help(&client).await;
    assert_eq!(rejected["session"]["state"], "rejected");
    assert_eq!(
        rejected["session"]["worker_process_id"],
        old["session"]["worker_process_id"]
    );
    assert_eq!(rejected["session"]["generation"], 1);
    assert_eq!(rejected["session"]["reconnect_required"], false);
    assert!(rejected["session"]["error"].is_string());
    std::fs::remove_file(&binary).unwrap();
    let missing = help(&client).await;
    assert!(
        missing["session"]["error"]
            .as_str()
            .unwrap()
            .contains("unavailable")
    );
    assert_eq!(
        missing["session"]["worker_process_id"],
        old["session"]["worker_process_id"]
    );
    assert_eq!(missing["runtime"]["reconnect_required"], false);
    replace(&binary, source);
    let recovered = help(&client).await;
    assert_eq!(recovered["session"]["generation"], 2);
    assert_eq!(recovered["session"]["state"], "current");
    assert_ne!(
        recovered["session"]["worker_process_id"],
        old["session"]["worker_process_id"]
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
async fn mcp_session_real_models_preserve_search_and_watch_across_replacement() {
    let models = std::path::PathBuf::from(
        std::env::var_os("MASH_TEST_MODELS_DIR").expect("explicit installed models"),
    );
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("mash");
    let source_binary = assert_cmd::cargo::cargo_bin!("mash");
    replace(&binary, source_binary);
    let source = directory.path().join("codex");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("session.jsonl"), concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"cache model\"}]}}\n"
    )).unwrap();
    let config = directory.path().join("node.toml");
    std::fs::write(&config, format!("version=1\n[providers.codex]\nroots=[{:?}]\n[index]\ndestination='local'\nall_history=true\n", source.to_str().unwrap())).unwrap();
    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("--config")
        .arg(config)
        .arg("--db")
        .arg(directory.path().join("index.sqlite3"))
        .arg("--models-dir")
        .arg(models)
        .arg("mcp");
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let indexed = client
        .call_tool(CallToolRequestParams::new("index"))
        .await
        .unwrap();
    assert_ne!(indexed.is_error, Some(true), "{indexed:?}");
    assert_eq!(
        indexed.structured_content.as_ref().unwrap()["indexed_messages"],
        1
    );
    let query = CallToolRequestParams::new("search").with_arguments(serde_json::json!({"query":"cache model","intent":"related-discussion","unit":"messages","local":true,"limit":5}).as_object().unwrap().clone());
    let before = client.call_tool(query.clone()).await.unwrap();
    assert_ne!(before.is_error, Some(true), "{before:?}");
    let before = before.structured_content.unwrap();
    assert_eq!(before["realized_mode"], "hybrid");
    assert_eq!(before["results"].as_array().unwrap().len(), 1);
    let watching = client
        .call_tool(
            CallToolRequestParams::new("watch_start").with_arguments(
                serde_json::json!({"debounce_ms":350})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(watching.is_error, Some(true));
    replace(&binary, source_binary);
    let switched = help(&client).await;
    assert_eq!(switched["session"]["generation"], 2);
    let watching = client
        .call_tool(CallToolRequestParams::new("watch_status"))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(watching["state"], "running");
    assert_eq!(watching["debounce_ms"], 350);
    let stopped = client
        .call_tool(CallToolRequestParams::new("watch_stop"))
        .await
        .unwrap();
    assert_ne!(stopped.is_error, Some(true));
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let state = client
                .call_tool(CallToolRequestParams::new("watch_status"))
                .await
                .unwrap()
                .structured_content
                .unwrap();
            if state["state"] == "stopped" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let after = client.call_tool(query).await.unwrap();
    assert_ne!(after.is_error, Some(true), "{after:?}");
    assert_eq!(
        before["results"],
        after.structured_content.unwrap()["results"]
    );
    client.cancel().await.unwrap();
}
