use super::*;

fn assert_rejects_opposite_archive(directory: &std::path::Path, archive: &std::path::Path) {
    let content = std::fs::read_to_string(archive).unwrap();
    let malformed = content
        .lines()
        .flat_map(|line| {
            let mut lines = vec![line.to_owned()];
            if line.contains("\"relation\":\"supports\"") {
                lines.push(
                    line.replace("\"relation\":\"supports\"", "\"relation\":\"contradicts\""),
                );
            }
            lines
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(archive, malformed).unwrap();
    let rejected = directory.join("rejected.sqlite3");
    let error = crate::storage::export::restore(archive, &rejected, true).unwrap_err();
    assert!(
        error.error.message.contains("opposite relationships"),
        "{error:?}"
    );
    if rejected.exists() {
        let rejected = Storage::open(&rejected).unwrap();
        assert!(
            rejected
                .records(&List::default())
                .unwrap()
                .records
                .is_empty()
        );
    }
}

#[test]
fn native_records_assertion_links_preserve_archive_ownership_and_reject_scope_changes() {
    let (directory, mut storage) = database();
    storage
        .connection
        .execute("UPDATE chats SET source_key=id", [])
        .unwrap();
    let first = storage
        .mutate_record(&create_request("first", decision()))
        .unwrap();
    let mut body = decision();
    body.statement = "Qdrant suits filtered retrieval".into();
    let second = storage
        .mutate_record(&create_request("second", body))
        .unwrap();
    let mut link = Mutation {
        operation_id: "support".into(),
        change: Change::Link {
            expected: expected(&first),
            target: expected(&second),
            relation: records::AssertionRelation::Supports,
        },
    };
    storage
        .connection
        .execute(
            "UPDATE records SET owner='foreign' WHERE id=?1",
            [&second.record_id],
        )
        .unwrap();
    assert!(storage.mutate_record(&link).is_err());
    storage
        .connection
        .execute(
            r#"UPDATE records SET owner=?2,scope='{"kind":"personal"}' WHERE id=?1"#,
            params![second.record_id, owner(&storage.connection).unwrap()],
        )
        .unwrap();
    assert!(storage.mutate_record(&link).is_err());
    storage
        .connection
        .execute(
            "UPDATE records SET scope=?2 WHERE id=?1",
            params![
                second.record_id,
                encode(&Scope::Repository {
                    identity: "repo".into()
                })
                .unwrap()
            ],
        )
        .unwrap();
    storage.mutate_record(&link).unwrap();
    assert!(!storage.record(&first.record_id, false).unwrap().conflicted);
    let archive = directory.path().join("assertions.jsonl");
    crate::storage::export::write(&directory.path().join("index.sqlite3"), &archive).unwrap();
    for keep in [false, true] {
        let path = directory.path().join(if keep {
            "restored.sqlite3"
        } else {
            "copied.sqlite3"
        });
        crate::storage::export::restore(&archive, &path, keep).unwrap();
        let copy = Storage::open(&path).unwrap();
        let page = copy.records(&List::default()).unwrap();
        assert_eq!(page.records.len(), 2);
        assert!(
            page.records
                .iter()
                .all(|record| record.links.len() == 1 && !record.conflicted)
        );
    }
    assert_rejects_opposite_archive(directory.path(), &archive);

    // A new operation may not reuse a now-stale endpoint revision.
    storage
        .mutate_record(&Mutation {
            operation_id: "retire".into(),
            change: Change::Forget {
                expected: expected(&second),
            },
        })
        .unwrap();
    link.operation_id = "after-forget".into();
    assert!(storage.mutate_record(&link).is_err());
}

#[test]
fn native_records_explicit_contradictions_are_symmetric_and_excluded_from_default_recall() {
    let (_directory, mut storage) = database();
    let first = storage
        .mutate_record(&create_request("first", decision()))
        .unwrap();
    let mut other = decision();
    other.statement = "Use SQLite instead".into();
    let second = storage
        .mutate_record(&create_request("second", other))
        .unwrap();
    let link = Mutation {
        operation_id: "link".into(),
        change: Change::Link {
            expected: Expected {
                id: first.record_id.clone(),
                revision: 1,
            },
            target: Expected {
                id: second.record_id.clone(),
                revision: 1,
            },
            relation: records::AssertionRelation::Contradicts,
        },
    };
    let receipt = storage.mutate_record(&link).unwrap();
    assert_eq!(
        encode(&receipt).unwrap(),
        encode(&storage.mutate_record(&link).unwrap()).unwrap()
    );
    for id in [&first.record_id, &second.record_id] {
        let record = storage.record(id, false).unwrap();
        assert!(record.conflicted);
        assert_eq!(record.links.len(), 1);
        assert_eq!(record.revisions[0].confirmations, 0);
    }
    let mut request: records::recall::Request = serde_json::from_value(
        serde_json::json!({"scope":{"kind":"repository","identity":"repo"}}),
    )
    .unwrap();
    assert!(
        storage
            .recall_records(&request, crate::redaction::Policy::ON)
            .unwrap()
            .records
            .is_empty()
    );
    request.include_review = true;
    let recalled = storage
        .recall_records(&request, crate::redaction::Policy::ON)
        .unwrap();
    assert_eq!(recalled.records.len(), 2);
    assert!(recalled.records.iter().all(|item| item.conflicted));
    let mut stale = link.clone();
    stale.operation_id = "stale".into();
    if let Change::Link { target, .. } = &mut stale.change {
        target.revision = 2;
    }
    assert!(storage.mutate_record(&stale).is_err());
    let mut opposite = link.clone();
    opposite.operation_id = "opposite".into();
    if let Change::Link { relation, .. } = &mut opposite.change {
        *relation = records::AssertionRelation::Supports;
    }
    assert!(storage.mutate_record(&opposite).is_err());
    // Ordinary correction does not silently resolve an identity-level disagreement.
    let mut body = decision();
    body.rationale = "Corrected wording".into();
    let correction = Mutation {
        operation_id: "correct".into(),
        change: Change::Revise {
            expected: Expected {
                id: first.record_id.clone(),
                revision: 1,
            },
            body,
            evidence: vec![citation("yes", "yes")],
        },
    };
    storage.mutate_record(&correction).unwrap();
    assert!(storage.record(&first.record_id, false).unwrap().conflicted);
    let forgotten = Mutation {
        operation_id: "forget".into(),
        change: Change::Forget {
            expected: Expected {
                id: second.record_id,
                revision: 1,
            },
        },
    };
    storage.mutate_record(&forgotten).unwrap();
    assert!(!storage.record(&first.record_id, false).unwrap().conflicted);
}
