use super::*;

#[cfg(unix)]
#[test]
fn runtime_inventory_is_unverified_when_its_relative_database_cannot_be_resolved() {
    const CHILD: &str = "MASH_TEST_RUNTIME_DELETED_CWD";
    if std::env::var_os(CHILD).is_some() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("removed");
        std::fs::create_dir(&cwd).unwrap();
        std::env::set_current_dir(&cwd).unwrap();
        std::fs::remove_dir(cwd).unwrap();
        let report = inspect(&Paths {
            db: Some("relative.sqlite3".into()),
            ..Paths::default()
        });
        std::env::set_current_dir(root.path()).unwrap();
        assert!(!report.complete);
        assert!(report.processes.is_empty());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runtime::tests::runtime_inventory_is_unverified_when_its_relative_database_cannot_be_resolved", "--nocapture"])
        .env(CHILD, "1")
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn runtime_admission_and_inventory_follow_executable_replacement() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("mash");
    std::fs::write(&executable, "old").unwrap();
    let paths = Paths {
        db: Some(root.path().join("index.sqlite3")),
        ..Paths::default()
    };
    let lease = Lease::fixture(&paths, &executable);
    lease.ensure_current().unwrap();
    let report = inspect(&paths);
    assert!(report.complete);
    assert_eq!(report.processes.len(), 1);
    assert_eq!(report.processes[0].state, State::Current);
    let replacement = root.path().join("replacement");
    std::fs::write(&replacement, "new").unwrap();
    std::fs::rename(replacement, &executable).unwrap();
    assert!(lease.ensure_current().is_err());
    assert_eq!(inspect(&paths).processes[0].state, State::Superseded);
    std::fs::remove_file(&executable).unwrap();
    assert!(lease.ensure_current().is_err());
    assert_eq!(inspect(&paths).processes[0].state, State::Unverified);
    drop(lease);
    assert!(inspect(&paths).processes.is_empty());
}

#[cfg(unix)]
#[test]
fn runtime_checks_invocation_symlink_and_in_place_changes() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let new = root.path().join("new");
    let executable = root.path().join("mash");
    std::fs::write(&old, "original").unwrap();
    std::fs::write(&new, "replacement").unwrap();
    std::os::unix::fs::symlink(&old, &executable).unwrap();
    let paths = Paths {
        db: Some(root.path().join("index.sqlite3")),
        ..Paths::default()
    };
    let lease = Lease::fixture(&paths, &executable);
    lease.ensure_current().unwrap();
    let link = root.path().join("next-link");
    std::os::unix::fs::symlink(&new, &link).unwrap();
    std::fs::rename(link, &executable).unwrap();
    assert!(lease.ensure_current().is_err());
    let lease = Lease::fixture(&paths, &old);
    std::fs::write(&old, "changed in place").unwrap();
    assert!(lease.ensure_current().is_err());
}

#[test]
fn runtime_inventory_ignores_abandoned_records_and_reports_unverifiable_evidence() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths {
        db: Some(root.path().join("absent/index.sqlite3")),
        ..Paths::default()
    };
    assert!(inspect(&paths).complete);
    assert!(!root.path().join("absent").exists());
    let directory = root.path().join("records");
    std::fs::create_dir(&directory).unwrap();
    let malformed = directory.join("malformed");
    std::fs::write(&malformed, "not-json").unwrap();
    assert!(inspect_directory(&directory).complete);
    let locked = File::open(&malformed).unwrap();
    locked.try_lock().unwrap();
    assert!(!inspect_directory(&directory).complete);
    drop(locked);
    std::fs::write(&malformed, vec![b' '; 16_385]).unwrap();
    assert!(!inspect_directory(&directory).complete);
}

#[test]
fn runtime_registration_does_not_recapture_a_replaced_identity() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("mash");
    std::fs::write(&executable, "running").unwrap();
    let running = identity(&executable).unwrap();
    std::fs::write(&executable, "new release").unwrap();
    let lease = Lease::create(root.path(), executable, running, Kind::Mcp).unwrap();
    assert!(lease.ensure_current().is_err());
}

#[test]
fn runtime_inventory_distinguishes_a_different_unchanged_build_from_replacement() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("old-release");
    std::fs::write(&executable, "running").unwrap();
    let paths = Paths {
        db: Some(root.path().join("index.sqlite3")),
        ..Paths::default()
    };
    let mut lease = Lease::fixture(&paths, &executable);
    lease.record.build_revision = "another-build".into();
    let record_path = std::fs::read_dir(directory(&paths).unwrap())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(record_path, serde_json::to_vec(&lease.record).unwrap()).unwrap();
    assert_eq!(inspect(&paths).processes[0].state, State::DifferentBuild);
    lease.ensure_current().unwrap();
}
