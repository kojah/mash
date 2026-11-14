//! `mash init` against a redirected home. Unix only: Windows resolves Mash's
//! directories through the Known Folders API, not HOME/XDG variables, and its
//! `mash init` is covered by the Windows workflow on a real profile.
#![cfg(unix)]

use super::*;

/// A home with Claude Code history and stand-in agent CLIs that log calls.
fn fixture(registered: bool) -> (tempfile::TempDir, Vec<(&'static str, String)>) {
    fixture_with(registered, "exit 0")
}

/// `plugin` runs as the stand-in's last line for plugin commands.
fn fixture_with(
    registered: bool,
    plugin: &str,
) -> (tempfile::TempDir, Vec<(&'static str, String)>) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let project = home.join(".claude/projects/p");
    std::fs::create_dir_all(&project).unwrap();
    let body = r#"{"type":"user","sessionId":"first","uuid":"m1","cwd":"/work","timestamp":"2026-10-28T09:00:00Z","message":{"role":"user","content":"initneedle schema change"}}"#;
    std::fs::write(project.join("first.jsonl"), format!("{body}\n")).unwrap();
    let bin = home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    for agent in ["claude", "codex"] {
        let script = bin.join(agent);
        let get = i32::from(!registered);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"{agent} $*\" >> '{}'\n[ \"$1 $2\" = \"mcp get\" ] && exit {get}\n[ \"$1\" = plugin ] && {{ {plugin}; }}\nexit 0\n",
                home.join("calls.log").display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let env = vec![
        ("HOME", home.display().to_string()),
        (
            "XDG_CONFIG_HOME",
            home.join(".config").display().to_string(),
        ),
        (
            "XDG_DATA_HOME",
            home.join(".local/share").display().to_string(),
        ),
        ("PATH", format!("{}:/usr/bin:/bin", bin.display())),
    ];
    (root, env)
}

fn init(env: &[(&str, String)], arguments: &[&str]) -> Value {
    let output = Command::cargo_bin("mash")
        .unwrap()
        .arg("init")
        .args(arguments)
        .env_clear()
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
#[ignore = "requires disposable Qdrant"]
fn init_configures_registers_and_indexes_detected_history() {
    let (root, env) = fixture(false);
    let home = root.path();
    let value = init(&env, &["--no-index"]);
    let config = home.join(".mash/config.toml");
    assert_eq!(value["config"]["written"], true, "{value}");
    assert_eq!(value["config"]["search"], "exact-mention");
    assert_eq!(
        value["config"]["providers"]["claude-code"][0],
        home.join(".claude/projects").display().to_string()
    );
    assert!(value["config"]["providers"].get("codex").is_none());
    configure_qdrant(&config);
    let indexed = init(&env, &["--no-agents"]);
    assert_eq!(indexed["index"]["indexed_messages"], 1, "{indexed}");
    for agent in &value["agents"].as_array().unwrap()[..] {
        assert_eq!(agent["mcp"]["state"], "done", "{value}");
        assert_eq!(agent["plugin"]["state"], "done", "{value}");
        // Each step names the command it ran, so users can see what changed.
        assert!(
            agent["mcp"]["command"]
                .as_str()
                .unwrap()
                .contains(" mcp add "),
            "{value}"
        );
    }
    let calls = std::fs::read_to_string(home.join("calls.log")).unwrap();
    assert!(
        calls.contains("claude mcp add --scope user mash -- "),
        "{calls}"
    );
    assert!(
        calls.contains("claude plugin marketplace add mashium/mash"),
        "{calls}"
    );
    assert!(calls.contains("claude plugin install mash@mash"), "{calls}");
    assert!(calls.contains("codex plugin add mash@mash"), "{calls}");
    assert!(
        calls
            .lines()
            .any(|line| line.starts_with("codex mcp add mash -- ") && line.ends_with(" mcp"))
    );

    let search = Command::cargo_bin("mash")
        .unwrap()
        .args(["search", "initneedle", "--local"])
        .env_clear()
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&search.stdout).contains("\"chat_id\":\"first\""));

    // A second run keeps the configuration and existing registrations.
    std::fs::write(
        &config,
        std::fs::read_to_string(&config).unwrap() + "# edited\n",
    )
    .unwrap();
    let (_registered_home, registered) = fixture(true);
    let mut env = env;
    env.retain(|(key, _)| *key != "PATH");
    env.extend(registered.into_iter().filter(|(key, _)| *key == "PATH"));
    let again = init(&env, &["--no-index"]);
    assert_eq!(again["config"]["written"], false);
    assert!(again.get("index").is_none());
    assert_eq!(
        again["agents"][0]["mcp"]["state"], "already-present",
        "{again}"
    );
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .ends_with("# edited\n")
    );

    let forced = init(&env, &["--force", "--no-index", "--no-agents"]);
    assert_eq!(forced["config"]["written"], true);
    assert_eq!(forced["agents"], serde_json::json!([]));
    assert!(home.join(".mash/config.toml.previous").exists());
    assert_eq!(
        forced["config"]["previous"],
        home.join(".mash/config.toml.previous")
            .display()
            .to_string()
    );
    cleanup_local_gold(&home.join(".mash/mash.sqlite3"), "local");
}

#[test]
fn init_reports_why_a_plugin_could_not_be_installed() {
    // Offline, adding the marketplace fails and installing then cannot find
    // the plugin; the second error alone would not say why.
    let (_root, env) = fixture_with(
        false,
        r#"if [ "$2" = marketplace ]; then echo 'getaddrinfo ENOTFOUND github.com' >&2; else echo 'plugin not found' >&2; fi; exit 1"#,
    );
    let value = init(&env, &["--no-index"]);
    for agent in &value["agents"].as_array().unwrap()[..] {
        assert_eq!(agent["plugin"]["state"], "failed", "{value}");
        let message = agent["plugin"]["message"].as_str().unwrap();
        assert!(message.contains("plugin not found"), "{message}");
        assert!(message.contains("ENOTFOUND github.com"), "{message}");
    }
    // The next steps say what still works and how to finish, per agent.
    let next = value["next"].to_string();
    for agent in ["claude-code", "codex"] {
        assert!(
            next.contains(&format!("{agent}: the Mash MCP server is registered")),
            "{next}"
        );
    }
    assert!(next.contains("Restart your agents"), "{next}");
}

#[test]
fn init_memory_backfill_is_explicit_and_agent_exit_does_not_certify_a_review() {
    let (root, env) = fixture(false);
    init(&env, &["--no-index", "--no-agents"]);
    let database = root.path().join(".mash/mash.sqlite3");
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    seed_readiness_database(&database, None);
    let connection = Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE chats SET updated_at=9223372036854775807,source_fingerprint=?1",
            ["a".repeat(64)],
        )
        .unwrap();
    let normal = init(&env, &["--yes", "--no-index", "--no-agents"]);
    assert!(normal.get("memory").is_none());
    let calls = root.path().join("calls.log");
    assert!(
        !calls.exists(),
        "ordinary init must not start a backfill agent"
    );

    for agent in ["codex", "claude-code"] {
        let incomplete = init(
            &env,
            &[
                "--yes",
                "--no-index",
                "--no-agents",
                "--backfill-memory",
                agent,
            ],
        );
        assert_eq!(incomplete["memory"]["complete"], false, "{incomplete}");
        assert_eq!(incomplete["memory"]["reviewed"], 0);
        assert!(
            incomplete["memory"]["message"]
                .as_str()
                .unwrap()
                .contains("without completing")
        );
        let checkpoints: i64 = connection
            .query_row("SELECT count(*) FROM record_checkpoints", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(checkpoints, 0, "exit zero must not mark a review complete");
    }
    // Simulate the durable result of a previously completed extraction. An
    // explicit resume skips it without assigning anything to another agent.
    connection.execute("INSERT INTO record_checkpoints(scope,chat_id,source_fingerprint) VALUES(?1,'session',?2)", rusqlite::params![r#"{"kind":"chat","id":"session"}"#, "a".repeat(64)]).unwrap();
    let previous = std::fs::read_to_string(&calls).unwrap();
    let resumed = init(
        &env,
        &[
            "--yes",
            "--no-index",
            "--no-agents",
            "--backfill-memory",
            "codex",
        ],
    );
    assert_eq!(resumed["memory"]["complete"], true, "{resumed}");
    assert_eq!(resumed["memory"]["reviewed"], 0);
    assert_eq!(std::fs::read_to_string(calls).unwrap(), previous);
}

#[test]
fn init_without_history_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let env = [
        ("HOME", root.path().display().to_string()),
        (
            "XDG_CONFIG_HOME",
            root.path().join(".config").display().to_string(),
        ),
        ("PATH", "/usr/bin:/bin".to_owned()),
    ];
    // Without --no-index the Qdrant check would come first.
    let output = Command::cargo_bin("mash")
        .unwrap()
        .args(["init", "--no-index"])
        .env_clear()
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no supported agent history"));
    assert!(!root.path().join(".mash/config.toml").exists());
}

#[test]
fn init_yes_accepts_defaults_and_preserves_existing_configuration() {
    let (root, env) = fixture(false);
    let result = init(&env, &["--yes", "--no-index", "--no-agents"]);
    assert_eq!(result["config"]["written"], true);
    assert_eq!(result["agents"], serde_json::json!([]));
    assert!(!root.path().join("calls.log").exists());
    let path = root.path().join(".mash/config.toml");
    let original = std::fs::read_to_string(&path).unwrap() + "\n# keep me\n";
    std::fs::write(&path, &original).unwrap();
    let result = init(&env, &["--yes", "--no-index", "--no-agents"]);
    assert_eq!(result["config"]["written"], false);
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

/// Run `mash` in the fixture home, returning the exit code, stdout and the
/// last stderr line (progress events precede a final error).
fn mash(env: &[(&str, String)], arguments: &[&str]) -> (i32, Value, Value) {
    let output = Command::cargo_bin("mash")
        .unwrap()
        .args(arguments)
        .env_clear()
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .timeout(HANG_GUARD)
        .output()
        .unwrap();
    let parse = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .last()
            .map_or(Value::Null, |line| serde_json::from_str(line).unwrap())
    };
    (
        output.status.code().unwrap(),
        parse(&output.stdout),
        parse(&output.stderr),
    )
}

/// A configuration whose Qdrant endpoints refuse connections.
fn unreachable_qdrant(home: &std::path::Path, url: &str) -> std::path::PathBuf {
    let config = home.join(".mash/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        format!(
            "version = 1\n[providers.claude-code]\nroots = [{}]\n[views.build]\nrepresentations = 'lexical-only'\n[views.query]\nintent = 'exact-mention'\n[qdrant]\nurl = '{url}'\ngrpc_url = 'http://127.0.0.1:9'\n",
            toml::Value::String(home.join(".claude/projects").display().to_string())
        ),
    )
    .unwrap();
    config
}

#[test]
fn init_without_qdrant_changes_nothing_and_status_says_to_start_it() {
    let (root, env) = fixture(false);
    let home = root.path();
    let config = unreachable_qdrant(home, "http://127.0.0.1:9");
    let before = std::fs::read_to_string(&config).unwrap();
    let (code, stdout, error) = mash(&env, &["init", "--yes"]);
    assert_eq!(code, 11, "{error}");
    assert!(stdout.is_null(), "{stdout}");
    assert_eq!(error["error"]["kind"], "qdrant-unavailable", "{error}");
    assert_eq!(error["error"]["recommended_action"], "start-qdrant");
    let message = error["error"]["message"].as_str().unwrap();
    for part in [
        "changed nothing",
        "http://127.0.0.1:9/",
        // R2 S-002: the route to reading history before Qdrant runs.
        "To read your chats before Qdrant runs, run `mash init --no-index`, then `mash index`",
        "`mash list-chats` and `mash view <message id>` read it without Qdrant.",
        "Then rerun `mash init`.",
        "Guide: https://getmash.io/installation/#start-the-local-search-service",
    ] {
        assert!(message.contains(part), "{message}");
    }
    // One sequence: start Qdrant, then one next step.
    assert_eq!(message.matches("Then ").count(), 1, "{message}");
    assert!(!message.contains("retry"), "{message}");
    // No agent CLI ran, no history was read and the configuration is intact.
    assert!(!home.join("calls.log").exists());
    assert!(!home.join(".mash/mash.sqlite3").exists());
    assert_eq!(std::fs::read_to_string(&config).unwrap(), before);

    // Status keeps liveness apart from readiness and does not loop on index.
    let (code, status, _) = mash(&env, &["status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["ready"], false);
    assert_eq!(status["local_qdrant"]["state"], "unreachable", "{status}");
    assert_eq!(status["local_qdrant"]["url"], "http://127.0.0.1:9");
    assert_eq!(status["recommended_action"], "start-qdrant");
    let (code, installation, _) = mash(&env, &["status", "--installation"]);
    assert_eq!(code, 0, "{installation}");
    let finding = installation["resources"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["component"] == "qdrant")
        .unwrap();
    assert_eq!(finding["state"], "missing", "{finding}");
    let next = installation["summary"]["next_action"].as_str().unwrap();
    assert!(
        next.starts_with("Start Qdrant at http://127.0.0.1:9"),
        "{next}"
    );
    assert!(next.contains("mash index"), "{next}");

    // Setup without indexing still works, and says what indexing needs.
    let (code, configured, _) = mash(&env, &["init", "--yes", "--no-index"]);
    assert_eq!(code, 0, "{configured}");
    let next = configured["next"].as_array().unwrap();
    let step = next
        .iter()
        .filter_map(Value::as_str)
        .find(|step| step.contains("Could not reach Qdrant at http://127.0.0.1:9/"))
        .unwrap_or_else(|| panic!("{next:?}"));
    for part in [
        "To read your chats now, run `mash index`: it saves your history before it needs Qdrant",
        "`mash list-chats` and `mash view <message id>` read it without Qdrant.",
        "Then run `mash index` to make search current.",
    ] {
        assert!(step.contains(part), "{step}");
    }
    assert_eq!(step.matches("Then ").count(), 1, "{step}");
    assert!(!configured["next"].to_string().contains("search works"));

    reads_before_and_after_a_failed_index(&env);
}

/// What a configured machine without Qdrant says before and after `index`.
fn reads_before_and_after_a_failed_index(env: &[(&str, String)]) {
    // P1 S-002: configured but not indexed, reads name `mash index` and
    // Qdrant rather than sending the user back to `mash init`.
    let (code, stdout, error) = mash(env, &["list-chats"]);
    assert_eq!(code, 3, "{error}");
    assert!(stdout.is_null(), "{stdout}");
    assert_eq!(error["error"]["kind"], "database-missing", "{error}");
    assert_eq!(error["error"]["recommended_action"], "index", "{error}");
    let message = error["error"]["message"].as_str().unwrap();
    for part in [
        "Mash is configured, but nothing is indexed yet",
        "Run `mash index`: it saves your history",
        "needs Qdrant at http://127.0.0.1:9",
    ] {
        assert!(message.contains(part), "{message}");
    }
    assert!(!message.contains("mash init"), "{message}");

    // R2 S-001: index commits history, then fails on Qdrant, and says the
    // saved chats are readable without it.
    let (code, stdout, error) = mash(env, &["index"]);
    assert_eq!(code, 11, "{error}");
    assert!(stdout.is_null(), "{stdout}");
    assert_eq!(error["error"]["kind"], "qdrant-unavailable", "{error}");
    assert_eq!(error["error"]["retryable"], true, "{error}");
    assert_eq!(error["error"]["recommended_action"], "start-qdrant");
    let message = error["error"]["message"].as_str().unwrap();
    for part in [
        "Could not reach Qdrant at http://127.0.0.1:9/",
        "Your 1 indexed chat is saved on this machine",
        "`mash list-chats` and `mash view <message id>` (MCP `list_chats` and `view`) read it without Qdrant.",
        "Then run `mash index` again to make search current.",
    ] {
        assert!(message.contains(part), "{message}");
    }
    assert_eq!(message.matches("Then ").count(), 1, "{message}");

    // B-010: search still fails closed but names the saved chats too.
    let (code, stdout, error) = mash(env, &["search", "initneedle"]);
    assert_eq!(code, 11, "{error}");
    assert!(stdout.is_null(), "{stdout}");
    assert_eq!(error["error"]["kind"], "qdrant-unavailable", "{error}");
    assert_eq!(error["error"]["recommended_action"], "start-qdrant");
    let message = error["error"]["message"].as_str().unwrap();
    for part in [
        "Your 1 indexed chat is saved on this machine",
        "`mash list-chats` and `mash view <message id>`",
        "Then search again.",
    ] {
        assert!(message.contains(part), "{message}");
    }
    let (code, listed, error) = mash(env, &["list-chats"]);
    assert_eq!(code, 0, "{error}");
    assert_eq!(listed["chats"].as_array().unwrap().len(), 1, "{listed}");
}

/// Answers every HTTP request as Qdrant 1.20 would answer its root, so the
/// reachability check passes while every later collection request fails.
fn version_only_qdrant() -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut request = [0_u8; 8192];
            let _ = stream.read(&mut request);
            let body = r#"{"title":"qdrant - vector search engine","version":"1.20.0"}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

#[test]
fn init_keeps_its_setup_report_when_the_first_index_fails() {
    let (root, env) = fixture(false);
    let home = root.path();
    unreachable_qdrant(home, &version_only_qdrant());
    let (code, report, error) = mash(&env, &["init", "--yes"]);
    assert_ne!(code, 0, "{report}");
    // The error still fails the command, on stderr as any failure would.
    assert_eq!(report["index_error"], error["error"], "{report}");
    assert!(report.get("index").is_none(), "{report}");
    assert_eq!(report["config"]["written"], false, "{report}");
    for agent in report["agents"].as_array().unwrap() {
        assert_eq!(agent["mcp"]["state"], "done", "{report}");
    }
    let next = report["next"].as_array().unwrap();
    let first = next[0].as_str().unwrap();
    assert!(first.starts_with("Setup was saved"), "{first}");
    assert!(first.contains("mash index"), "{first}");
    assert!(report["next"].to_string().contains("Restart your agents"));
    assert!(
        std::fs::read_to_string(home.join("calls.log"))
            .unwrap()
            .contains("mcp add")
    );
}

/// Stand-in agent CLIs that keep a Mash registration in the agents' real
/// user settings files, as the real CLIs do, so status inspects what init did.
/// Each registration starts out naming the missing executable `/old/mash`.
fn stale_fixture(refuse_remove: bool) -> (tempfile::TempDir, Vec<(&'static str, String)>) {
    let (root, env) = fixture(false);
    let home = root.path();
    let claude = home.join(".claude.json");
    let codex = home.join(".codex/config.toml");
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    let write_claude =
        r#"printf '{"mcpServers":{"mash":{"type":"stdio","command":"%s","args":["mcp"]}}}' "$1""#;
    let write_codex = r#"printf '[mcp_servers.mash]\ncommand = "%s"\nargs = ["mcp"]\n' "$1""#;
    for (agent, settings, register) in [
        ("claude", &claude, write_claude),
        ("codex", &codex, write_codex),
    ] {
        std::fs::write(
            home.join("bin").join(agent),
            format!(
                "#!/bin/sh\necho \"{agent} $*\" >> '{log}'\ncase \"$1 $2\" in\n\"mcp get\") [ -f '{settings}' ]; exit $?;;\n\"mcp remove\") [ {refuse} = 1 ] && {{ echo 'remove refused' >&2; exit 1; }}; rm -f '{settings}'; exit 0;;\n\"mcp add\") while [ \"$1\" != -- ]; do shift; done; shift; {register} > '{settings}'; exit 0;;\nesac\nexit 0\n",
                log = home.join("calls.log").display(),
                settings = settings.display(),
                refuse = i32::from(refuse_remove),
            ),
        )
        .unwrap();
    }
    std::fs::write(
        &claude,
        r#"{"mcpServers":{"mash":{"type":"stdio","command":"/old/mash","args":["mcp"]}}}"#,
    )
    .unwrap();
    std::fs::write(
        &codex,
        "[mcp_servers.mash]\ncommand = \"/old/mash\"\nargs = [\"mcp\"]\n",
    )
    .unwrap();
    (root, env)
}

#[test]
fn init_replaces_a_stale_registration_and_status_expects_recorded_agents() {
    let (root, env) = stale_fixture(false);
    let home = root.path();
    let value = init(&env, &["--no-index"]);
    for (index, agent, remove) in [
        (0, "claude-code", "claude mcp remove --scope user mash"),
        (1, "codex", "codex mcp remove mash"),
    ] {
        let outcome = &value["agents"][index];
        assert_eq!(outcome["agent"], agent, "{value}");
        assert_eq!(outcome["mcp"]["state"], "replaced", "{value}");
        assert_eq!(outcome["mcp"]["stale"], "/old/mash", "{value}");
        assert_eq!(outcome["undo"][0], remove, "{value}");
        assert!(!outcome["files"].as_array().unwrap().is_empty(), "{value}");
    }
    assert_eq!(
        value["config"]["recorded_agents"],
        serde_json::json!(["claude-code", "codex"])
    );
    assert!(
        value["next"].to_string().contains("To undo agent setup"),
        "{value}"
    );
    let calls = std::fs::read_to_string(home.join("calls.log")).unwrap();
    let removed = calls.find("claude mcp remove --scope user mash").unwrap();
    let added = calls.find("claude mcp add --scope user mash -- ").unwrap();
    assert!(removed < added, "{calls}");
    assert!(calls.contains("codex mcp remove mash\n"), "{calls}");
    let config = home.join(".mash/config.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("[agents]"), "{text}");

    // The replacement runs this binary, so a rerun leaves it in place.
    let again = init(&env, &["--no-index"]);
    assert_eq!(
        again["agents"][0]["mcp"]["state"], "already-present",
        "{again}"
    );
    assert!(again["config"].get("recorded_agents").is_none(), "{again}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), text);

    // The stand-ins install no plugin files: the MCP tools work, so the
    // agents are connected for MCP only, and that still needs attention.
    let (code, report, _) = mash(&env, &["status", "--installation"]);
    assert_eq!(code, 1, "{report}");
    for client in report["summary"]["clients"].as_array().unwrap() {
        assert_eq!(client["connection"], "mcp-only", "{report}");
        assert!(
            client["next_action"]
                .as_str()
                .unwrap()
                .contains("work without the plugin"),
            "{report}"
        );
    }

    // A removed registration for a recorded agent is broken, not CLI-only use.
    std::fs::remove_file(home.join(".claude.json")).unwrap();
    let (code, report, _) = mash(&env, &["status", "--installation"]);
    assert_eq!(code, 1, "{report}");
    let claude = &report["summary"]["clients"][1];
    assert_eq!(claude["agent"], "claude-code");
    assert_eq!(claude["expected"], true);
    assert_eq!(claude["connection"], "none");
    assert!(
        claude["next_action"]
            .as_str()
            .unwrap()
            .starts_with("Run `claude mcp add --scope user mash -- "),
        "{report}"
    );
    assert_eq!(report["clients"][1]["expectation"], "recorded");
}

#[test]
fn init_fails_with_the_commands_when_a_stale_registration_cannot_be_replaced() {
    let (_root, env) = stale_fixture(true);
    let (code, value, _) = mash(&env, &["init", "--no-index"]);
    assert_eq!(code, 1, "{value}");
    let mcp = &value["agents"][1]["mcp"];
    assert_eq!(mcp["state"], "stale", "{value}");
    assert_eq!(mcp["stale"], "/old/mash");
    assert!(mcp["message"].as_str().unwrap().contains("remove refused"));
    let command = mcp["command"].as_str().unwrap();
    assert!(
        command.contains("codex mcp remove mash && ") && command.contains(" mcp add mash -- "),
        "{command}"
    );
    let next = value["next"].to_string();
    assert!(
        next.contains("codex: the Mash MCP registration runs /old/mash"),
        "{next}"
    );
    assert!(value["config"].get("recorded_agents").is_none(), "{value}");
}
