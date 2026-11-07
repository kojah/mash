use super::*;

const CHILD_ROOT: &str = "MASH_MIGRATION_INTERRUPTION_ROOT";
const CHILD_PHASE: &str = "MASH_MIGRATION_INTERRUPTION_PHASE";

fn ready_and_wait(root: &Path) -> ! {
    std::fs::write(root.join("ready"), b"transaction boundary reached").unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[test]
fn migration_interruption_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    let phase = std::env::var(CHILD_PHASE).unwrap();
    let mut connection = Connection::open(root.join("index.sqlite3")).unwrap();
    if phase == "during" {
        let signal_root = root.clone();
        connection
            .authorizer(Some(move |context: rusqlite::hooks::AuthContext<'_>| {
                if matches!(
                    context.action,
                    rusqlite::hooks::AuthAction::CreateTable {
                        table_name: "record_revisions"
                    }
                ) {
                    // record_owner and records have already been created in
                    // the real migration transaction, which has not committed.
                    ready_and_wait(&signal_root);
                }
                rusqlite::hooks::Authorization::Allow
            }))
            .unwrap();
    }
    crate::storage::schema::initialize_silver(&mut connection).unwrap();
    assert_eq!(phase, "after");
    ready_and_wait(&root);
}

struct RunningMigration(std::process::Child);

impl Drop for RunningMigration {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn snapshot(connection: &Connection) -> (String, String, i64, String) {
    connection.query_row(
        "SELECT d.instance,p.instance,s.revision,m.content FROM local_database_identity d,shared_publication p,silver_state s,messages m",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap()
}

#[test]
fn killed_migrations_recover_atomic_schema_and_retained_history() {
    for phase in ["during", "after"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("index.sqlite3");
        let connection = baseline(&path);
        let before = snapshot(&connection);
        drop(connection);
        let mut child = RunningMigration(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage::tests::schema::interruption::migration_interruption_child",
                    "--nocapture",
                ])
                .env(CHILD_ROOT, directory.path())
                .env(CHILD_PHASE, phase)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + crate::test_support::HANG_GUARD;
        while !directory.path().join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before {phase} boundary"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "child did not reach {phase} boundary"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if phase == "during" {
            let error = Storage::open(&path)
                .err()
                .expect("live migration owns admission");
            assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
        }
        // Child::kill is SIGKILL on Unix and TerminateProcess on Windows.
        // No Rust/SQLite destructors run in the interrupted process.
        child.0.kill().unwrap();
        let status = child.0.wait().unwrap();
        assert!(!status.success());
        let connection = Connection::open(&path).unwrap();
        crate::storage::packed::register(&connection).unwrap();
        assert_eq!(snapshot(&connection), before);
        let expected_version = if phase == "during" {
            62
        } else {
            SCHEMA_VERSION
        };
        assert_eq!(user_version(&path), expected_version);
        let tables: i64 = connection.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('record_owner','records','record_revisions')",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(tables, if phase == "during" { 0 } else { 3 });
        drop(connection);
        let recovered = Storage::open(&path).unwrap();
        assert_eq!(user_version(&path), SCHEMA_VERSION);
        assert_eq!(snapshot(&recovered.connection), before);
    }
}
