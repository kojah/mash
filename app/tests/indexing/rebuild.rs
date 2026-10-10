use super::*;

#[derive(Clone, Copy)]
enum State {
    Missing,
    Unverified,
    Pending,
    Incomplete,
}

#[test]
fn views_rebuild_rejects_missing_and_uncertified_silver_without_creating_or_mutating_it() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("index.sqlite3");
    let models = directory.path().join("models");
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
    create_valid_model_marker(&models);
    for state in [
        State::Missing,
        State::Unverified,
        State::Pending,
        State::Incomplete,
    ] {
        let existing = !matches!(state, State::Missing);
        if matches!(state, State::Unverified) {
            seed_readiness_database(&database, Some(""));
        }
        if matches!(state, State::Pending | State::Incomplete) {
            let connection = Connection::open(&database).unwrap();
            let scan_state = if matches!(state, State::Pending) {
                0
            } else {
                2
            };
            connection.execute("INSERT INTO silver_scans(provider,interpretation_version,state) VALUES('codex',1,?1) ON CONFLICT(provider) DO UPDATE SET state=excluded.state", [scan_state]).unwrap();
        }
        let before = existing.then(|| std::fs::read(&database).unwrap());
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models.to_str().unwrap(),
                "index",
                "--rebuild-local-views",
                "--full",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(output.stdout, [] as [u8; 0]);
        assert_eq!(
            error_kind(&output),
            if existing {
                "usage"
            } else {
                "database-missing"
            }
        );
        if let Some(before) = before {
            assert_eq!(std::fs::read(&database).unwrap(), before);
        } else {
            assert!(!database.exists());
        }
    }
}

#[test]
fn views_rebuild_conflicts_with_source_selectors_and_watch() {
    for selector in [
        vec!["--provider", "claude-code"],
        vec!["--since-days", "7"],
        vec!["--all-history"],
        vec!["--watch"],
    ] {
        Command::cargo_bin("mash")
            .unwrap()
            .args(["index", "--rebuild-local-views"])
            .args(selector)
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used"));
    }
}
