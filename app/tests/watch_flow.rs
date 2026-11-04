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
        Self::start_binary(root, extra, assert_cmd::cargo::cargo_bin!("mash"))
    }

    fn start_binary(root: &Path, extra: &[&str], executable: &Path) -> Self {
        let mut child = std::process::Command::new(executable)
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
            .recv_timeout(HANG_GUARD)
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

#[cfg(unix)]
#[test]
fn runtime_upgrade_stops_idle_and_standby_cli_watchers() {
    let root = tempfile::tempdir().unwrap();
    let sources = root.path().join("history");
    std::fs::create_dir(&sources).unwrap();
    history(&sources.join("session.jsonl"), "before upgrade");
    configure(root.path(), &sources);
    create_valid_model_marker(&root.path().join("models"));
    let executable = root.path().join("mash");
    std::os::unix::fs::symlink(assert_cmd::cargo::cargo_bin!("mash"), &executable).unwrap();
    let mut leader = Watching::start_binary(root.path(), &[], &executable);
    assert_eq!(leader.next()["indexed_messages"], 1);
    let mut standby = Watching::start_binary(root.path(), &[], &executable);
    let deadline = std::time::Instant::now() + HANG_GUARD;
    loop {
        // Each live runtime holds a readable record and a sibling lock.
        if std::fs::read_dir(root.path().join("index.sqlite3.runtimes"))
            .unwrap()
            .count()
            == 4
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "standby registered");
        std::thread::sleep(Duration::from_millis(25));
    }
    let replacement = root.path().join("replacement");
    std::fs::write(&replacement, "new executable identity").unwrap();
    std::fs::rename(replacement, executable).unwrap();
    for watcher in [&mut leader, &mut standby] {
        loop {
            if let Some(status) = watcher.child.try_wait().unwrap() {
                assert!(
                    !status.success(),
                    "superseded watch exits with a typed error"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "superseded watch stops"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    assert_eq!(
        std::fs::read_dir(root.path().join("index.sqlite3.runtimes"))
            .unwrap()
            .count(),
        0
    );
    let connection = super::support::open_silver(root.path().join("index.sqlite3"));
    let content: String = connection
        .query_row(
            "SELECT content FROM objects WHERE origin='provider' AND object_role='entry'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(content, "before upgrade");
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

// Bounding the write burst needs SIGSTOP/SIGCONT, which Windows lacks.
#[cfg(unix)]
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
    // Queue one burst before the watcher can process it. Sleeping between writes
    // does not bound their spacing on a loaded host and can exceed the debounce.
    #[cfg(unix)]
    assert!(
        std::process::Command::new("kill")
            .args(["-STOP", &watcher.child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    for content in ["one", "two", "three"] {
        history(&source, content);
    }
    #[cfg(unix)]
    assert!(
        std::process::Command::new("kill")
            .args(["-CONT", &watcher.child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(watcher.next()["changed_messages"], 1);
    let connection = super::support::open_silver(root.join("index.sqlite3"));
    let content: String = connection
        .query_row(
            "SELECT content FROM objects WHERE origin='provider' AND object_role='entry'",
            [],
            |r| r.get(0),
        )
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
    let connection = super::support::open_silver(root.join("index.sqlite3"));
    let content: String = connection
        .query_row(
            "SELECT content FROM objects WHERE origin='provider' AND object_role='entry'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(content, "new root");
    watcher.quiet();
    history(&sources.join("session.jsonl"), "excluded old root");
    watcher.quiet();
}

#[test]
fn mcp_indexes_only_on_explicit_request_and_stops_when_session_closes() {
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
    assert!(
        !root.join("index.sqlite3").exists(),
        "MCP startup does not ingest"
    );
    send(
        &mut stdin,
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{}}}),
    );
    let indexed = watcher.next();
    assert_eq!(indexed["id"], 2);
    assert_ne!(indexed["result"]["isError"], true);
    send(
        &mut stdin,
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"status","arguments":{}}}),
    );
    let response = watcher.next();
    assert_eq!(response["id"], 3);
    assert_eq!(response["result"]["structuredContent"]["messages"], 1);
    drop(stdin);
    let deadline = std::time::Instant::now() + HANG_GUARD;
    loop {
        if let Some(status) = watcher.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(std::time::Instant::now() < deadline, "MCP exits on EOF");
        std::thread::sleep(Duration::from_millis(25));
    }
    watcher.quiet(); // No CLI response may follow the MCP shutdown.
}

#[cfg(unix)]
struct Elected {
    watching: Watching,
    roles: Receiver<String>,
}

#[cfg(unix)]
impl Elected {
    fn start(root: &Path) -> Self {
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let lines = |stream: Box<dyn std::io::Read + Send>| {
            let (sender, receiver) = channel::<Value>();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines() {
                    let Ok(value) = serde_json::from_str(&line.unwrap_or_default()) else {
                        continue;
                    };
                    if sender.send(value).is_err() {
                        break;
                    }
                }
            });
            receiver
        };
        let output = lines(Box::new(child.stdout.take().unwrap()));
        let events = lines(Box::new(child.stderr.take().unwrap()));
        let (sender, roles) = channel();
        std::thread::spawn(move || {
            for event in events {
                if event["event"] == "watch-role"
                    && sender
                        .send(event["role"].as_str().unwrap().to_owned())
                        .is_err()
                {
                    break;
                }
            }
        });
        Self {
            watching: Watching { child, output },
            roles,
        }
    }

    fn role(&self) -> String {
        self.roles
            .recv_timeout(HANG_GUARD)
            .expect("watch role within deadline")
    }

    fn signal(&self, name: &str) {
        assert!(
            std::process::Command::new("kill")
                .args([name, &self.watching.child.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
    }
}

#[cfg(unix)]
#[test]
fn killed_leader_fails_over_to_a_standby_that_indexes_changes_from_the_gap() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sources = root.join("history");
    std::fs::create_dir(&sources).unwrap();
    let source = sources.join("session.jsonl");
    history(&source, "before failover");
    configure(root, &sources);
    create_valid_model_marker(&root.join("models"));
    let mut leader = Elected::start(root);
    assert_eq!(leader.role(), "leader");
    assert_eq!(leader.watching.next()["indexed_messages"], 1);
    let standby = Elected::start(root);
    assert_eq!(standby.role(), "standby");
    // A standby cannot index before it holds the lock.
    assert!(standby.watching.output.try_recv().is_err());

    // Freeze the idle leader so it cannot see the change, then kill it while it
    // still holds the lock: nothing watches between the write and the takeover.
    leader.signal("-STOP");
    history(&source, "written while leaderless");
    leader.signal("-KILL");
    leader.watching.child.wait().unwrap();

    assert_eq!(standby.role(), "leader");
    assert_eq!(standby.watching.next()["changed_messages"], 1);
    let content: String = super::support::open_silver(root.join("index.sqlite3"))
        .query_row(
            "SELECT content FROM objects WHERE origin='provider' AND object_role='entry'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(content, "written while leaderless");
}
