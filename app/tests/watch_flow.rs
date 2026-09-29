use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use super::*;

struct Watching {
    child: Child,
    output: Receiver<Value>,
}

impl Watching {
    fn start(root: &Path, extra: &[&str]) -> Self {
        let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("mash"))
            .args([
                "--config",
                root.join("config.toml").to_str().unwrap(),
                "--db",
                root.join("index.sqlite3").to_str().unwrap(),
                "--models-dir",
                root.join("models").to_str().unwrap(),
                "index",
                "--watch",
                "--debounce-ms",
                "200",
            ])
            .args(extra)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if sender.send(value).is_err() {
                    break;
                }
            }
        });
        Self { child, output }
    }

    fn next(&self) -> Value {
        self.output
            .recv_timeout(Duration::from_secs(10))
            .expect("watch refresh within deadline")
    }

    fn quiet(&self) {
        assert!(
            self.output
                .recv_timeout(Duration::from_millis(700))
                .is_err(),
            "unexpected extra refresh"
        );
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn history(path: &Path, content: &str) {
    std::fs::write(path, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"watch-session\"}}}}\n{{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call_output\",\"call_id\":\"tool\",\"output\":{}}}}}\n", serde_json::to_string(content).unwrap())).unwrap();
}

fn configure(root: &Path, sources: &Path) {
    std::fs::write(
        root.join("config.toml"),
        toml::to_string(&toml::toml! {
            version = 1
            providers = {codex = {roots = [(sources.to_str().unwrap())]}}
            index = {destination = "local",all_history = true}
        })
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn watcher_coalesces_writes_tracks_atomic_replacement_and_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sources = root.join("history");
    std::fs::create_dir(&sources).unwrap();
    let source = sources.join("session.jsonl");
    history(&source, "initial");
    configure(root, &sources);
    create_valid_model_marker(&root.join("models"));
    let watcher = Watching::start(root, &[]);
    assert_eq!(watcher.next()["indexed_messages"], 1);
    watcher.quiet();
    for content in ["one", "two", "three"] {
        history(&source, content);
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(watcher.next()["changed_messages"], 1);
    let connection = Connection::open(root.join("index.sqlite3")).unwrap();
    let content: String = connection
        .query_row("SELECT content FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(content, "three");
    watcher.quiet();
    std::fs::write(sources.join("notes.txt"), "unrelated").unwrap();
    watcher.quiet();
    let temporary = sources.join("replacement.tmp");
    history(&temporary, "replacement");
    std::fs::rename(temporary, &source).unwrap();
    assert_eq!(watcher.next()["changed_messages"], 1);
    std::fs::remove_file(source).unwrap();
    assert_eq!(watcher.next()["indexed_messages"], 0);
    watcher.quiet();
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-INT", &watcher.child.id().to_string()])
            .status()
            .unwrap();
        assert_eq!(watcher.next()["watching"], false);
    }
}

#[test]
fn watcher_recovers_missing_roots_and_reloads_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sources = root.join("missing");
    configure(root, &sources);
    create_valid_model_marker(&root.join("models"));
    let watcher = Watching::start(root, &[]);
    std::thread::sleep(Duration::from_millis(500));
    std::fs::create_dir(&sources).unwrap();
    history(&sources.join("session.jsonl"), "created later");
    assert_eq!(watcher.next()["indexed_messages"], 1);
    let next = root.join("other");
    std::fs::create_dir(&next).unwrap();
    history(&next.join("session.jsonl"), "new root");
    configure(root, &next);
    watcher.next();
    let connection = Connection::open(root.join("index.sqlite3")).unwrap();
    let content: String = connection
        .query_row("SELECT content FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(content, "new root");
    watcher.quiet();
    history(&sources.join("session.jsonl"), "excluded old root");
    watcher.quiet();
}

#[test]
fn mcp_watcher_keeps_stdout_for_protocol_and_stops_when_session_closes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sources = root.join("history");
    std::fs::create_dir(&sources).unwrap();
    history(&sources.join("session.jsonl"), "MCP watched output");
    configure(root, &sources);
    create_valid_model_marker(&root.join("models"));
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("mash"))
        .args([
            "mcp",
            "--config",
            root.join("config.toml").to_str().unwrap(),
            "--db",
            root.join("index.sqlite3").to_str().unwrap(),
            "--models-dir",
            root.join("models").to_str().unwrap(),
            "--watch",
            "--debounce-ms",
            "200",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, output) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else {
                break;
            };
            let value: Value = serde_json::from_str(&line).expect("stdout contains only JSON RPC");
            if sender.send(value).is_err() {
                break;
            }
        }
    });
    let mut watcher = Watching { child, output };
    let send = |stdin: &mut std::process::ChildStdin, value: Value| {
        serde_json::to_writer(&mut *stdin, &value).unwrap();
        writeln!(stdin).unwrap();
        stdin.flush().unwrap();
    };
    send(
        &mut stdin,
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"watch-test","version":"1"}}}),
    );
    assert_eq!(watcher.next()["id"], 1);
    send(
        &mut stdin,
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let indexed = Connection::open_with_flags(
            root.join("index.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .is_ok_and(|db| {
            db.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
                .ok()
                == Some(1)
        });
        if indexed {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "MCP background indexing completes"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    send(
        &mut stdin,
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"status","arguments":{}}}),
    );
    let response = watcher.next();
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"]["structuredContent"]["messages"], 1);
    drop(stdin);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = watcher.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "MCP watcher exits on EOF"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    watcher.quiet(); // No CLI response may follow the MCP shutdown.
}
