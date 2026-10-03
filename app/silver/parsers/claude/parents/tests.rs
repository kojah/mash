use super::*;

fn disk() -> Parents {
    Parents {
        limit: 0,
        ..Parents::default()
    }
}

fn store(parents: &Parents) -> &Store {
    match &parents.state {
        State::Disk(store) => store,
        _ => panic!("expected disk ancestry"),
    }
}

#[test]
fn ancestry_states_replacements_and_exact_keys_survive_spill() {
    let mut parents = Parents {
        limit: ENTRY_BYTES + 5,
        ..Parents::default()
    };
    parents
        .insert("first", Ancestry::Known(ParentLink::Root))
        .unwrap();
    for _ in 0..10 {
        parents.insert("first", Ancestry::Ambiguous).unwrap();
    }
    assert!(matches!(parents.state, State::Inline { .. }));
    parents
        .insert("second", Ancestry::Known(ParentLink::Missing))
        .unwrap();
    assert_eq!(parents.get("first").unwrap(), Some(Ancestry::Ambiguous));
    let values = [
        Ancestry::Known(ParentLink::Missing),
        Ancestry::Known(ParentLink::Root),
        Ancestry::Known(ParentLink::Parent("a\0é".to_owned())),
        Ancestry::Ambiguous,
    ];
    for key in ["", "x", "X", "x\0", "é", "e\u{301}"] {
        for value in &values {
            parents.insert(key, value.clone()).unwrap();
            assert_eq!(parents.get(key).unwrap().as_ref(), Some(value));
        }
    }
    assert_eq!(parents.get("absent").unwrap(), None);
    assert_eq!(
        parents.get("second").unwrap(),
        Some(Ancestry::Known(ParentLink::Missing))
    );
    let store = store(&parents);
    assert_eq!(
        store
            .connection
            .query_row("PRAGMA cache_size", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        -1024
    );
    assert_eq!(
        store
            .connection
            .query_row("PRAGMA mmap_size", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let directory = std::path::Path::new(store.connection.path().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            directory.metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            directory
                .join("parents.sqlite3")
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
    drop(parents);
    assert!(!directory.exists());
}

#[test]
fn failed_writes_poison_ancestry_and_remove_scratch() {
    let mut parents = disk();
    parents
        .insert("first", Ancestry::Known(ParentLink::Root))
        .unwrap();
    let directory = std::path::Path::new(store(&parents).connection.path().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    store(&parents)
        .connection
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    assert!(parents.insert("second", Ancestry::Ambiguous).is_err());
    assert!(matches!(parents.state, State::Failed));
    assert!(parents.get("first").is_err());
    assert!(
        parents
            .insert("first", Ancestry::Known(ParentLink::Root))
            .is_err()
    );
    assert!(!directory.exists());
}

#[test]
fn damaged_disk_rows_and_read_failures_cannot_be_missing_ancestry() {
    let mut parents = disk();
    parents
        .insert("first", Ancestry::Known(ParentLink::Root))
        .unwrap();
    store(&parents)
        .connection
        .execute_batch(
            "PRAGMA ignore_check_constraints=ON;
         UPDATE parents SET state=99;",
        )
        .unwrap();
    assert!(parents.get("first").is_err());
    assert!(parents.visit(|_, _| Ok(())).is_err());
    store(&parents)
        .connection
        .execute_batch("UPDATE parents SET state=2,parent=x'ff'")
        .unwrap();
    assert!(parents.get("first").is_err());
    assert!(parents.visit(|_, _| Ok(())).is_err());
    store(&parents)
        .connection
        .execute_batch("DROP TABLE parents")
        .unwrap();
    assert!(parents.get("absent").is_err());
    assert!(parents.visit(|_, _| Ok(())).is_err());
}

#[test]
fn visitation_rejects_non_utf8_staged_identity() {
    let mut parents = disk();
    parents
        .insert("first", Ancestry::Known(ParentLink::Root))
        .unwrap();
    store(&parents)
        .connection
        .execute_batch("UPDATE parents SET id=x'ff'")
        .unwrap();
    assert!(parents.visit(|_, _| Ok(())).is_err());
}

#[test]
fn staged_visitation_is_exact_short_circuits_and_clear_removes_scratch() {
    for limit in [0, INLINE_BYTES] {
        let mut parents = Parents::with_limit(limit);
        for key in ["é", "x\0", "X", ""] {
            parents
                .insert(key, Ancestry::Known(ParentLink::Root))
                .unwrap();
        }
        let mut visited = Vec::new();
        parents
            .visit(|key, value| {
                visited.push((key.to_owned(), value));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            visited
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            ["", "X", "x\0", "é"]
        );
        assert!(
            visited
                .iter()
                .all(|(_, value)| *value == Ancestry::Known(ParentLink::Root))
        );
        let mut count = 0;
        assert!(
            parents
                .visit(|_, _| {
                    count += 1;
                    Err(AppError::internal("visitor failed"))
                })
                .is_err()
        );
        assert_eq!(count, 1);
        let directory = match &parents.state {
            State::Disk(store) => Some(
                std::path::Path::new(store.connection.path().unwrap())
                    .parent()
                    .unwrap()
                    .to_owned(),
            ),
            _ => None,
        };
        parents.clear();
        assert_eq!(parents.get("X").unwrap(), None);
        assert!(directory.is_none_or(|directory| !directory.exists()));
        parents
            .insert("again", Ancestry::Known(ParentLink::Missing))
            .unwrap();
        assert_eq!(
            parents.get("again").unwrap(),
            Some(Ancestry::Known(ParentLink::Missing))
        );
    }
}

#[test]
fn compaction_restores_only_proven_replay_in_inline_and_disk_stores() {
    use super::super::replay::Replay;
    use crate::silver::json::Record;
    for limit in [0, INLINE_BYTES] {
        for proven in [false, true] {
            let mut parents = Parents {
                limit,
                ..Parents::default()
            };
            let mut replay = Replay::with_limit(limit);
            let records = [
                r#"{"type":"user","uuid":"root","parentUuid":null}"#,
                r#"{"type":"user","uuid":"answer","parentUuid":"root"}"#,
                r#"{"type":"progress","uuid":"tail","parentUuid":"answer"}"#,
                r#"{"type":"user","uuid":"answer","parentUuid":null}"#,
            ];
            for json in records {
                replay
                    .observe(&serde_json::from_str::<Record>(json).unwrap(), &mut parents)
                    .unwrap();
            }
            assert_eq!(parents.get("answer").unwrap(), Some(Ancestry::Ambiguous));
            let boundary: Record = serde_json::from_str(if proven {
                r#"{"type":"system","subtype":"compact_boundary","uuid":"boundary","parentUuid":null,"logicalParentUuid":"tail","compactMetadata":{"preservedSegment":{"tailUuid":"tail"}}}"#
            } else {
                r#"{"type":"system","subtype":"compact_boundary","uuid":"boundary","parentUuid":null,"logicalParentUuid":"tail","compactMetadata":{"preservedSegment":{"tailUuid":"wrong"}}}"#
            }).unwrap();
            replay.observe(&boundary, &mut parents).unwrap();
            assert_eq!(
                parents.get("answer").unwrap(),
                Some(if proven {
                    Ancestry::Known(ParentLink::Parent("root".to_owned()))
                } else {
                    Ancestry::Ambiguous
                })
            );
        }
    }
}
