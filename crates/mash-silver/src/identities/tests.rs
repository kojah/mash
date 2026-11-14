use super::*;

fn small() -> Identities {
    Identities {
        limit: ENTRY_BYTES + 5,
        ..Identities::default()
    }
}

fn store(identities: &Identities) -> &Store {
    match &identities.state {
        State::Disk(store) => store,
        _ => panic!("must spill"),
    }
}

fn directory(identities: &Identities) -> &std::path::Path {
    std::path::Path::new(store(identities).connection.path().unwrap())
        .parent()
        .unwrap()
}

#[test]
fn exact_duplicate_detection_survives_spill_and_cleanup() {
    let mut identities = small();
    assert!(identities.insert("first").unwrap());
    assert!(!identities.insert("first").unwrap());
    assert!(matches!(identities.state, State::Inline { .. }));
    assert!(identities.insert("second").unwrap());
    let path = directory(&identities).to_path_buf();
    assert!(!identities.insert("first").unwrap());
    assert!(!identities.insert("second").unwrap());
    let distinct = ["First", "first\0", "é", "e\u{301}", "", "x"];
    for id in distinct {
        assert!(identities.insert(id).unwrap(), "must distinguish {id:?}");
    }
    for id in distinct {
        assert!(
            !identities.insert(id).unwrap(),
            "must reject duplicate {id:?}"
        );
    }
    assert_eq!(
        store(&identities)
            .connection
            .query_row("PRAGMA cache_size", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        -1024
    );
    assert_eq!(
        store(&identities)
            .connection
            .query_row("PRAGMA mmap_size", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(identities);
    assert!(!path.exists());
}

#[test]
fn failed_disk_validation_poisoned_state_cleans_up_instead_of_accepting_ids() {
    let mut identities = small();
    identities.insert("first").unwrap();
    identities.insert("second").unwrap();
    let path = directory(&identities).to_path_buf();
    store(&identities)
        .connection
        .execute_batch("PRAGMA query_only=ON;")
        .unwrap();
    assert!(identities.insert("third").is_err());
    assert!(matches!(identities.state, State::Failed));
    assert!(identities.insert("third").is_err());
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn identity_scratch_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let mut identities = small();
    identities.insert("first").unwrap();
    identities.insert("second").unwrap();
    let directory = directory(&identities);
    assert_eq!(
        directory.metadata().unwrap().permissions().mode() & 0o777,
        0o700
    );
    let file = directory.join("ids.sqlite3");
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o077, 0);
}
