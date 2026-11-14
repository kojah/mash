use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
#[ignore = "subprocess helper for cache construction locking"]
fn cache_subprocess_loader() {
    let Some(root) = std::env::var_os("MASH_TEST_COREML_CACHE_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let (_model, _lease) = load(&root, |directory| {
        let busy = directory.join(".building");
        let _writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&busy)
            .map_err(|error| BuildError::Compile(AppError::io(error)))?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        fs::write(directory.join("package.bin"), b"complete").unwrap();
        fs::remove_file(busy).unwrap();
        Ok(())
    })
    .unwrap();
}

#[test]
fn separate_processes_share_cache_without_overlapping_construction() {
    let root = tempfile::tempdir().unwrap();
    let spawn = || {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "semantic::coreml::cache::tests::cache_subprocess_loader",
                "--ignored",
            ])
            .env("MASH_TEST_COREML_CACHE_CHILD", root.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut first = spawn();
    let mut second = spawn();
    assert!(first.wait().unwrap().success());
    assert!(second.wait().unwrap().success());
    assert_eq!(
        fs::read(root.path().join("package.bin")).unwrap(),
        b"complete"
    );
    assert!(
        !root.path().join(MARKER).exists(),
        "overlapping constructors would trigger repair"
    );
}

#[test]
fn construction_is_serialized_for_independent_loaders() {
    let root = tempfile::tempdir().unwrap();
    let active = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let (model, _lease) = load(root.path(), |_| {
                        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                        Ok(42)
                    })
                    .unwrap();
                    assert_eq!(model, 42);
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
    });
}

#[test]
fn repair_keeps_loaded_generations_until_their_readers_finish() {
    let root = tempfile::tempdir().unwrap();
    let mut first = true;
    let (old, old_lease) = load(root.path(), |directory| {
        if first {
            first = false;
            return Err(BuildError::Compile(AppError::model(
                "incomplete legacy package",
            )));
        }
        fs::write(directory.join("model.bin"), b"ready").unwrap();
        Ok(directory.to_path_buf())
    })
    .unwrap();
    assert_ne!(old, root.path());
    let (new, new_lease) = load(root.path(), |directory| {
        if directory == old {
            return Err(BuildError::Compile(AppError::model("interrupted package")));
        }
        Ok(directory.to_path_buf())
    })
    .unwrap();
    assert_ne!(old, new);
    assert_eq!(fs::read(old.join("model.bin")).unwrap(), b"ready");
    assert_eq!(selected(root.path()).unwrap(), new);
    drop(old_lease);
    let (_, lease) = load(root.path(), |directory| Ok(directory.to_path_buf())).unwrap();
    assert!(!old.exists());
    assert!(new.exists());
    drop((lease, new_lease));
}

#[test]
fn input_errors_do_not_rotate_cache_and_repeated_compile_errors_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let mut calls = 0;
    let error = load::<()>(root.path(), |_| {
        calls += 1;
        Err(BuildError::Input(AppError::model("bad tokenizer")))
    })
    .err()
    .unwrap();
    assert_eq!(calls, 1);
    assert!(error.error.message.contains("bad tokenizer"));
    calls = 0;
    load::<()>(root.path(), |_| {
        calls += 1;
        Err(BuildError::Compile(AppError::model("unsupported model")))
    })
    .err()
    .unwrap();
    assert_eq!(calls, 2);
    assert!(!root.path().join(MARKER).exists());
    assert!(
        !fs::read_dir(root.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(PREFIX))
    );
}

#[cfg(unix)]
#[test]
fn read_only_retired_cache_does_not_prevent_loading_the_active_generation() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join(format!("{PREFIX}retired"));
    fs::create_dir(&old).unwrap();
    fs::write(old.join(LEASE), b"").unwrap();
    fs::write(old.join("model.bin"), b"retired").unwrap();
    fs::set_permissions(old.join(LEASE), fs::Permissions::from_mode(0o444)).unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o555)).unwrap();
    let active = root.path().join(format!("{PREFIX}active"));
    fs::create_dir(&active).unwrap();
    publish(root.path(), &active).unwrap();
    let result = load(root.path(), |directory| {
        assert_eq!(directory, active);
        Ok(())
    });
    // Restore permissions before assertions so temporary-directory cleanup also
    // works when the assertion fails. Root may have collected the directory.
    if old.exists() {
        fs::set_permissions(&old, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(old.join(LEASE), fs::Permissions::from_mode(0o644)).unwrap();
    }
    assert!(result.is_ok());
    assert_eq!(selected(root.path()).unwrap(), active);
}

#[test]
fn cache_marker_rejects_escape_paths_and_preserves_unknown_orphans() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(MARKER), "directory = '../outside'\n").unwrap();
    assert!(load::<()>(root.path(), |_| panic!("invalid pointer reached SDK")).is_err());
    fs::remove_file(root.path().join(MARKER)).unwrap();
    let orphan = root.path().join(format!("{PREFIX}unknown"));
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join("unknown.bin"), b"preserve").unwrap();
    load(root.path(), |_| Ok(())).unwrap();
    assert!(orphan.join("unknown.bin").exists());
}
