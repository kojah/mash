use super::*;
use crate::records::{DecisionState, FactState, PreferenceState};
mod extraction;
mod matches;
mod reading;
mod recall;

fn database() -> (tempfile::TempDir, Storage) {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::open(&directory.path().join("index.sqlite3")).unwrap();
    storage.connection.execute_batch("INSERT INTO chats(id,provider,project,repository) VALUES('chat','codex','/project','repo'),('other','codex','/other','other-repo');
        INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES
        ('proposal','chat',0,'assistant','Choose Qdrant for local search.'),
        ('yes','chat',1,'user','yes'),
        ('again','chat',2,'user','Still choose Qdrant.'),
        ('other-message','other',0,'user','another project');").unwrap();
    (directory, storage)
}

fn body(details: Details) -> Body {
    Body {
        statement: "Use Qdrant".into(),
        rationale: "filtered retrieval".into(),
        review: Review::Established,
        details,
        effective_at: None,
        recorded_by: "test agent (unverified attribution)".into(),
    }
}

fn decision() -> Body {
    body(Details::Decision {
        state: DecisionState::Accepted,
        alternatives: vec!["SQLite".into()],
        consequences: vec![],
    })
}

fn citation(message: &str, quote: &str) -> Citation {
    Citation {
        message_id: message.into(),
        quote: quote.into(),
        purpose: EvidenceUse::Endorsement,
        expected_fingerprint: None,
    }
}

fn create_request(operation: &str, body: Body) -> Mutation {
    Mutation {
        operation_id: operation.into(),
        change: Change::Create {
            scope: Scope::Repository {
                identity: "repo".into(),
            },
            body,
            evidence: vec![
                citation("proposal", "Choose Qdrant"),
                citation("yes", "yes"),
            ],
            on_duplicate: records::DuplicatePolicy::Report,
        },
    }
}

fn expected(receipt: &Receipt) -> Expected {
    Expected {
        id: receipt.record_id.clone(),
        revision: receipt.revision,
    }
}

#[test]
fn native_records_all_four_kinds_have_typed_states_and_short_approval_evidence() {
    let (_directory, mut storage) = database();
    let variants = [
        decision(),
        body(Details::Preference {
            state: PreferenceState::Active,
            conditions: vec!["local queries".into()],
        }),
        body(Details::Fact {
            state: FactState::Current,
            reported_by: "user".into(),
            valid_until: None,
        }),
        body(Details::OpenQuestion {
            state: QuestionState::Open,
            answer: None,
        }),
    ];
    for (index, body) in variants.into_iter().enumerate() {
        let created = storage
            .mutate_record(&create_request(&format!("create-{index}"), body.clone()))
            .unwrap();
        let record = storage.record(&created.record_id, true).unwrap();
        assert_eq!(record.revisions[0].body, body);
        assert!(record.revisions[0].evidence_current);
        for source in &record.revisions[0].evidence {
            assert_eq!(source.purpose, Some(EvidenceUse::Endorsement));
        }
        assert_eq!(
            record.revisions[0].evidence[0].source_role,
            Some(crate::role::Role::Assistant)
        );
        assert_eq!(
            record.revisions[0].evidence[1].source_role,
            Some(crate::role::Role::User)
        );
        assert_eq!(record.revisions[0].evidence[1].quote, "yes");
    }
    assert_eq!(storage.records(&List::default()).unwrap().records.len(), 4);
    assert!(
        serde_json::from_str::<Details>(
            r#"{"kind":"fact","state":"accepted","reported_by":"user","valid_until":null}"#
        )
        .is_err()
    );
}

#[test]
fn native_records_retry_receipts_and_failures_are_atomic() {
    let (_directory, mut storage) = database();
    let request = create_request("operation", decision());
    let first = storage.mutate_record(&request).unwrap();
    let second = storage.mutate_record(&request).unwrap();
    assert_eq!(encode(&first).unwrap(), encode(&second).unwrap());
    assert_eq!(storage.records(&List::default()).unwrap().records.len(), 1);
    let mut different = request.clone();
    if let Change::Create { body, .. } = &mut different.change {
        body.statement = "different".into();
    }
    assert_eq!(
        storage.mutate_record(&different).unwrap_err().error.kind,
        crate::ErrorKind::Conflict
    );
    storage.connection.execute_batch("CREATE TEMP TRIGGER interrupt_receipt BEFORE INSERT ON record_operations BEGIN SELECT RAISE(ABORT,'simulated interruption before receipt'); END;").unwrap();
    let mut interrupted_body = decision();
    interrupted_body.statement = "A separate conclusion".into();
    let failed = create_request("interrupted", interrupted_body);
    assert!(
        storage
            .mutate_record(&failed)
            .unwrap_err()
            .error
            .message
            .contains("simulated interruption before receipt")
    );
    assert_eq!(storage.records(&List::default()).unwrap().records.len(), 1);
    storage
        .connection
        .execute_batch("DROP TRIGGER interrupt_receipt")
        .unwrap();
    storage.mutate_record(&failed).unwrap();
    assert_eq!(storage.records(&List::default()).unwrap().records.len(), 2);
}

#[test]
fn native_records_candidates_scope_and_fingerprint_checks_do_not_infer_approval() {
    let (_directory, mut storage) = database();
    let mut request = create_request("candidate", decision());
    if let Change::Create { body, evidence, .. } = &mut request.change {
        evidence.pop();
        body.review = Review::Candidate;
    }
    let created = storage.mutate_record(&request).unwrap();
    assert!(
        storage
            .records(&List::default())
            .unwrap()
            .records
            .is_empty()
    );
    assert_eq!(
        storage.record(&created.record_id, false).unwrap().revisions[0]
            .body
            .review,
        Review::Candidate
    );
    request.operation_id = "no-approval".into();
    if let Change::Create { body, .. } = &mut request.change {
        body.review = Review::Established;
    }
    assert!(storage.mutate_record(&request).is_err());
    request.operation_id = "wrong-scope".into();
    if let Change::Create { evidence, .. } = &mut request.change {
        *evidence = vec![citation("other-message", "another project")];
    }
    assert!(storage.mutate_record(&request).is_err());
    request.operation_id = "wrong-fingerprint".into();
    if let Change::Create { evidence, .. } = &mut request.change {
        *evidence = vec![citation("yes", "yes")];
        evidence[0].expected_fingerprint = Some("outdated".into());
    }
    assert_eq!(
        storage.mutate_record(&request).unwrap_err().error.kind,
        crate::ErrorKind::Conflict
    );
}

#[test]
fn native_records_confirmation_requires_new_evidence_and_correction_resets_it() {
    let (_directory, mut storage) = database();
    let created = storage
        .mutate_record(&create_request("create", decision()))
        .unwrap();
    let mut request = Mutation {
        operation_id: "same-source".into(),
        change: Change::Confirm {
            expected: expected(&created),
            evidence: vec![citation("yes", "yes")],
        },
    };
    storage.mutate_record(&request).unwrap();
    assert_eq!(
        storage.record(&created.record_id, false).unwrap().revisions[0].confirmations,
        0
    );
    request.operation_id = "new-source".into();
    if let Change::Confirm { evidence, .. } = &mut request.change {
        *evidence = vec![citation("again", "Still choose Qdrant.")];
    }
    storage.mutate_record(&request).unwrap();
    storage.mutate_record(&request).unwrap();
    request.operation_id = "same-source-new-operation".into();
    storage.mutate_record(&request).unwrap();
    assert_eq!(
        storage.record(&created.record_id, false).unwrap().revisions[0].confirmations,
        1
    );
    let revision = Mutation {
        operation_id: "correct".into(),
        change: Change::Revise {
            expected: expected(&created),
            body: decision(),
            evidence: vec![citation("yes", "yes")],
        },
    };
    storage.mutate_record(&revision).unwrap();
    let history = storage.record(&created.record_id, true).unwrap();
    assert_eq!(history.revisions.len(), 2);
    assert_eq!(history.revisions[0].confirmations, 0);
    assert_eq!(history.revisions[1].confirmations, 1);
    assert_eq!(history.revisions[0].confirmation_history, []);
    let confirmations = &history.revisions[1].confirmation_history;
    assert_eq!(confirmations.len(), 1);
    assert_eq!(confirmations[0].message_id, "again");
    assert!(confirmations[0].confirmed_at > 0);
    assert_eq!(
        storage.record(&created.record_id, true).unwrap().revisions[1].confirmation_history,
        *confirmations
    );
}

#[test]
fn native_records_supersession_is_atomic_and_reversals_keep_three_identities() {
    let (_directory, mut storage) = database();
    let first = storage
        .mutate_record(&create_request("A", decision()))
        .unwrap();
    let mut alternate = decision();
    alternate.statement = "Use SQLite".into();
    let successor = Mutation {
        operation_id: "B".into(),
        change: Change::Supersede {
            expected: expected(&first),
            body: alternate,
            evidence: vec![citation("yes", "yes")],
        },
    };
    let second = storage.mutate_record(&successor).unwrap();
    let competing = Mutation {
        operation_id: "competitor".into(),
        ..successor
    };
    assert_eq!(
        storage.mutate_record(&competing).unwrap_err().error.kind,
        crate::ErrorKind::Conflict
    );
    let third = storage
        .mutate_record(&Mutation {
            operation_id: "A-again".into(),
            change: Change::Supersede {
                expected: expected(&second),
                body: decision(),
                evidence: vec![citation("again", "Still choose Qdrant.")],
            },
        })
        .unwrap();
    assert_ne!(first.record_id, third.record_id);
    assert_ne!(second.record_id, third.record_id);
    assert!(
        storage.record(&first.record_id, false).unwrap().revisions[0]
            .body
            .details
            .superseded()
    );
    let current = storage.records(&List::default()).unwrap();
    assert_eq!(current.records.len(), 1);
    assert_eq!(current.records[0].id, third.record_id);
}

#[test]
fn native_records_changed_or_unknown_source_role_requires_review() {
    let (_directory, mut storage) = database();
    let created = storage
        .mutate_record(&create_request("create", decision()))
        .unwrap();
    storage
        .connection
        .execute("UPDATE messages SET role='assistant' WHERE id='yes'", [])
        .unwrap();
    let record = storage.record(&created.record_id, false).unwrap();
    assert!(!record.revisions[0].evidence_current);
    assert_eq!(
        record.revisions[0].evidence[1].source_role,
        Some(crate::role::Role::User)
    );
    assert!(
        storage
            .records(&List::default())
            .unwrap()
            .records
            .is_empty()
    );
    storage
        .connection
        .execute("UPDATE messages SET role='user' WHERE id='yes'", [])
        .unwrap();
    assert!(storage.record(&created.record_id, false).unwrap().revisions[0].evidence_current);
    // Migrated evidence with unknown provenance must not acquire an inferred approval.
    storage
        .connection
        .execute(
            "UPDATE record_evidence SET source_role=NULL WHERE message_id='yes'",
            [],
        )
        .unwrap();
    assert!(!storage.record(&created.record_id, false).unwrap().revisions[0].evidence_current);
    assert!(
        storage
            .records(&List::default())
            .unwrap()
            .records
            .is_empty()
    );
    let retained = storage
        .records(&List {
            include_inactive: true,
            ..List::default()
        })
        .unwrap();
    assert_eq!(retained.records.len(), 1);
}

#[test]
fn native_records_source_refresh_marks_stale_but_forget_scrubs_all_revisions() {
    let (_directory, mut storage) = database();
    let request = create_request("create", decision());
    let created = storage.mutate_record(&request).unwrap();
    storage
        .connection
        .execute(
            "UPDATE messages SET content_packed='changed' WHERE id='yes'",
            [],
        )
        .unwrap();
    assert!(!storage.record(&created.record_id, false).unwrap().revisions[0].evidence_current);
    assert!(
        storage
            .records(&List::default())
            .unwrap()
            .records
            .is_empty()
    );
    assert!(storage.forget("chat").unwrap());
    assert!(storage.record(&created.record_id, true).is_err());
    for table in [
        "record_revisions",
        "record_evidence",
        "record_confirmations",
    ] {
        assert_eq!(
            storage
                .connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    // An old successful receipt cannot resurrect its forgotten record.
    storage.mutate_record(&request).unwrap();
    assert!(storage.record(&created.record_id, true).is_err());
}

#[test]
fn native_records_do_not_change_chat_projection_certificate() {
    let (_directory, mut storage) = database();
    let before = encode(&storage.silver_certificate().unwrap()).unwrap();
    let publication: i64 = storage
        .connection
        .query_row("SELECT revision FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    storage
        .mutate_record(&create_request("create", decision()))
        .unwrap();
    assert_eq!(
        encode(&storage.silver_certificate().unwrap()).unwrap(),
        before
    );
    assert!(
        storage
            .connection
            .query_row("SELECT revision FROM shared_publication", [], |row| row
                .get::<_, i64>(0))
            .unwrap()
            > publication
    );
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM record_changes", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn native_records_resolution_links_survive_retry_reopening_and_archive_remapping() {
    let (directory, mut storage) = database();
    storage
        .connection
        .execute("UPDATE chats SET source_key=id", [])
        .unwrap();
    let answer = storage
        .mutate_record(&create_request("answer", decision()))
        .unwrap();
    let question_body = body(Details::OpenQuestion {
        state: QuestionState::Open,
        answer: None,
    });
    let question = storage
        .mutate_record(&create_request("question", question_body.clone()))
        .unwrap();
    storage
        .connection
        .execute("DELETE FROM record_changes", [])
        .unwrap();
    let request = Mutation {
        operation_id: "resolve-with-link".into(),
        change: Change::Resolve {
            expected: expected(&question),
            answer: "Use Qdrant".into(),
            resolved_by: Some(expected(&answer)),
            evidence: vec![citation("yes", "yes")],
        },
    };
    let resolved = storage.mutate_record(&request).unwrap();
    assert_eq!(resolved.related.as_ref().unwrap().id, answer.record_id);
    assert_eq!(
        storage.mutate_record(&request).unwrap().revision,
        resolved.revision
    );
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM record_changes", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let linked = storage.record(&question.record_id, true).unwrap();
    assert_eq!(linked.links.len(), 1);
    assert_eq!(linked.links[0].source_id, answer.record_id);
    assert_eq!(linked.links[0].target_id, question.record_id);
    assert!(matches!(
        linked.links[0].relation,
        records::Relation::Resolves
    ));
    storage
        .mutate_record(&Mutation {
            operation_id: "reopen-linked".into(),
            change: Change::Revise {
                expected: expected(&resolved),
                body: question_body,
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap();
    let archive = directory.path().join("linked.jsonl");
    crate::storage::export::write(&directory.path().join("index.sqlite3"), &archive).unwrap();
    for keep in [true, false] {
        let path = directory.path().join(if keep {
            "restore-linked.sqlite3"
        } else {
            "copy-linked.sqlite3"
        });
        crate::storage::export::restore(&archive, &path, keep).unwrap();
        let restored = Storage::open(&path).unwrap();
        let page = restored.records(&List::default()).unwrap();
        assert_eq!(page.records.len(), 2);
        let reopened = page
            .records
            .iter()
            .find(|record| {
                matches!(
                    record.revisions[0].body.details,
                    Details::OpenQuestion { .. }
                )
            })
            .unwrap();
        assert_eq!(reopened.links.len(), 1);
        let answer_record = restored
            .record(&reopened.links[0].source_id, false)
            .unwrap();
        assert_eq!(answer_record.revisions[0].body.statement, "Use Qdrant");
        assert_eq!(reopened.id == question.record_id, keep);
        assert_eq!(answer_record.id == answer.record_id, keep);
    }
}

#[test]
fn native_records_resolution_rejects_stale_revisions_scope_owner_and_evidence() {
    let (_directory, mut storage) = database();
    let answer = storage
        .mutate_record(&create_request("answer", decision()))
        .unwrap();
    let question = storage
        .mutate_record(&create_request(
            "question",
            body(Details::OpenQuestion {
                state: QuestionState::Open,
                answer: None,
            }),
        ))
        .unwrap();
    let mut request = Mutation {
        operation_id: "resolve".into(),
        change: Change::Resolve {
            expected: expected(&question),
            answer: "Use Qdrant".into(),
            resolved_by: Some(Expected {
                id: answer.record_id.clone(),
                revision: 2,
            }),
            evidence: vec![citation("yes", "yes")],
        },
    };
    assert!(storage.mutate_record(&request).is_err());
    if let Change::Resolve { resolved_by, .. } = &mut request.change {
        *resolved_by = Some(expected(&answer));
    }
    for (sql, undo) in [
        (
            "UPDATE records SET scope='{\"kind\":\"personal\"}' WHERE kind='decision'",
            "UPDATE records SET scope='{\"kind\":\"repository\",\"identity\":\"repo\"}' WHERE kind='decision'",
        ),
        (
            "UPDATE records SET owner='foreign' WHERE kind='decision'",
            "UPDATE records SET owner=(SELECT id FROM record_owner) WHERE kind='decision'",
        ),
        (
            "UPDATE messages SET role='assistant' WHERE id='yes'",
            "UPDATE messages SET role='user' WHERE id='yes'",
        ),
        (
            "UPDATE record_revisions SET body=json_set(body,'$.review','candidate') WHERE record_id IN (SELECT id FROM records WHERE kind='decision')",
            "UPDATE record_revisions SET body=json_set(body,'$.review','established') WHERE record_id IN (SELECT id FROM records WHERE kind='decision')",
        ),
    ] {
        storage.connection.execute_batch(sql).unwrap();
        assert!(storage.mutate_record(&request).is_err(), "{sql}");
        assert_eq!(
            storage
                .record(&question.record_id, true)
                .unwrap()
                .current_revision,
            1
        );
        assert!(
            storage
                .record(&question.record_id, true)
                .unwrap()
                .links
                .is_empty()
        );
        storage.connection.execute_batch(undo).unwrap();
    }
    assert!(storage.mutate_record(&request).is_ok());
}

#[test]
fn native_records_question_resolution_reopening_owner_and_private_text_guards() {
    let (_directory, mut storage) = database();
    let question = body(Details::OpenQuestion {
        state: QuestionState::Open,
        answer: None,
    });
    let created = storage
        .mutate_record(&create_request("question", question.clone()))
        .unwrap();
    let resolved = storage
        .mutate_record(&Mutation {
            operation_id: "resolve".into(),
            change: Change::Resolve {
                expected: expected(&created),
                answer: "Use Qdrant".into(),
                resolved_by: None,
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap();
    assert!(
        storage
            .records(&List::default())
            .unwrap()
            .records
            .is_empty()
    );
    storage
        .mutate_record(&Mutation {
            operation_id: "reopen".into(),
            change: Change::Revise {
                expected: expected(&resolved),
                body: question,
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap();
    assert_eq!(storage.records(&List::default()).unwrap().records.len(), 1);
    let mut private = decision();
    private.rationale = "<private>canary</private>".into();
    assert!(
        storage
            .mutate_record(&create_request("private", private))
            .is_err()
    );
    let created = storage
        .mutate_record(&create_request("owned", decision()))
        .unwrap();
    storage
        .connection
        .execute(
            "UPDATE records SET owner='other-owner' WHERE id=?1",
            [&created.record_id],
        )
        .unwrap();
    assert!(
        storage
            .mutate_record(&Mutation {
                operation_id: "foreign".into(),
                change: Change::Forget {
                    expected: expected(&created)
                }
            })
            .is_err()
    );
    assert!(storage.record(&created.record_id, false).is_ok());
}

#[test]
fn native_records_old_approval_with_new_assistant_context_is_not_a_confirmation() {
    let (_directory, mut storage) = database();
    let created = storage
        .mutate_record(&create_request("create", decision()))
        .unwrap();
    storage.connection.execute_batch("INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('assistant-new','chat',3,'assistant','I repeat the old decision');").unwrap();
    let mut context = citation("assistant-new", "I repeat the old decision");
    context.purpose = EvidenceUse::Context;
    storage
        .mutate_record(&Mutation {
            operation_id: "not-confirmed".into(),
            change: Change::Confirm {
                expected: expected(&created),
                evidence: vec![citation("yes", "yes"), context],
            },
        })
        .unwrap();
    assert_eq!(
        storage.record(&created.record_id, false).unwrap().revisions[0].confirmations,
        0
    );
}

#[test]
fn native_records_archive_copy_and_identity_restore_preserve_history_and_links() {
    let (directory, mut storage) = database();
    storage
        .connection
        .execute("UPDATE chats SET source_key=id", [])
        .unwrap();
    let initial_request = create_request("first", decision());
    let initial = storage.mutate_record(&initial_request).unwrap();
    let mut replacement = decision();
    replacement.statement = "Use another engine".into();
    let second = storage
        .mutate_record(&Mutation {
            operation_id: "second".into(),
            change: Change::Supersede {
                expected: expected(&initial),
                body: replacement,
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap();
    storage
        .mutate_record(&Mutation {
            operation_id: "confirmation".into(),
            change: Change::Confirm {
                expected: expected(&second),
                evidence: vec![citation("again", "Still choose Qdrant.")],
            },
        })
        .unwrap();
    let archive = directory.path().join("canonical.jsonl");
    crate::storage::export::write(&directory.path().join("index.sqlite3"), &archive).unwrap();
    for keep in [false, true] {
        let path = directory.path().join(if keep {
            "restore.sqlite3"
        } else {
            "copy.sqlite3"
        });
        crate::storage::export::restore(&archive, &path, keep).unwrap();
        let mut imported = Storage::open(&path).unwrap();
        let active = imported.records(&List::default()).unwrap();
        assert_eq!(active.records.len(), 1);
        let record = &active.records[0];
        assert_eq!(record.revisions[0].body.statement, "Use another engine");
        assert_eq!(record.revisions[0].confirmations, 1);
        assert_eq!(record.revisions[0].confirmation_history.len(), 1);
        assert_eq!(
            record.revisions[0].confirmation_history[0].message_id,
            "again"
        );
        assert!(record.revisions[0].evidence_current);
        for source in &record.revisions[0].evidence {
            assert_eq!(source.purpose, Some(EvidenceUse::Endorsement));
            assert_eq!(source.source_role, Some(crate::role::Role::User));
        }
        assert_eq!(record.links.len(), 1);
        let predecessor = imported.record(&record.links[0].target_id, true).unwrap();
        assert_eq!(predecessor.revisions.len(), 2);
        assert!(predecessor.revisions[0].body.details.superseded());
        if keep {
            assert_eq!(record.id, second.record_id);
            assert_eq!(record.owner, owner(&storage.connection).unwrap());
            assert_eq!(
                imported.mutate_record(&initial_request).unwrap().record_id,
                initial.record_id
            );
        } else {
            assert_ne!(record.id, second.record_id);
            assert_ne!(record.owner, owner(&storage.connection).unwrap());
            assert!(imported.record(&second.record_id, false).is_err());
            assert_eq!(
                imported
                    .connection
                    .query_row("SELECT count(*) FROM record_operations", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
}

#[test]
fn native_records_archive_rejects_incomplete_history_without_committing_records() {
    let (directory, mut storage) = database();
    storage
        .connection
        .execute("UPDATE chats SET source_key=id", [])
        .unwrap();
    storage
        .mutate_record(&create_request("create", decision()))
        .unwrap();
    let archive = directory.path().join("canonical.jsonl");
    crate::storage::export::write(&directory.path().join("index.sqlite3"), &archive).unwrap();
    let contents = std::fs::read_to_string(&archive).unwrap();
    let altered = contents
        .lines()
        .filter(|line| {
            !matches!(
                serde_json::from_str::<crate::silver::archive::Record>(line).unwrap(),
                crate::silver::archive::Record::Native {
                    item: crate::records::transfer::Row::Evidence { .. }
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&archive, altered).unwrap();
    let destination = directory.path().join("rejected.sqlite3");
    assert!(crate::storage::export::restore(&archive, &destination, true).is_err());
    let check = Storage::open(&destination).unwrap();
    assert_eq!(check.counts().unwrap().chats, 0);
    assert!(check.records(&List::default()).unwrap().records.is_empty());
}

#[test]
fn native_records_archive_rejects_receipts_for_missing_records() {
    let (directory, mut storage) = database();
    storage
        .connection
        .execute("UPDATE chats SET source_key=id", [])
        .unwrap();
    storage
        .mutate_record(&create_request("create", decision()))
        .unwrap();
    let archive = directory.path().join("canonical.jsonl");
    crate::storage::export::write(&directory.path().join("index.sqlite3"), &archive).unwrap();
    let contents = std::fs::read_to_string(&archive).unwrap();
    let mut altered = String::new();
    for line in contents.lines() {
        let mut row: crate::silver::archive::Record = serde_json::from_str(line).unwrap();
        if let crate::silver::archive::Record::Native {
            item: crate::records::transfer::Row::Operation { result, .. },
        } = &mut row
        {
            result.record_id = "missing-record".into();
        }
        altered.push_str(&serde_json::to_string(&row).unwrap());
        altered.push('\n');
    }
    std::fs::write(&archive, altered).unwrap();
    for keep in [false, true] {
        let destination = directory.path().join(format!("rejected-{keep}.sqlite3"));
        assert!(crate::storage::export::restore(&archive, &destination, keep).is_err());
        let check = Storage::open(&destination).unwrap();
        assert_eq!(check.counts().unwrap().chats, 0);
    }
}

fn shared_rows(storage: &Storage, after: Option<i64>) -> Vec<crate::records::transfer::Row> {
    let mut rows = Vec::new();
    transfer::visit_shared(
        &storage.connection,
        after,
        crate::redaction::Policy::ON,
        |row| {
            rows.push(row);
            Ok(())
        },
    )
    .unwrap();
    rows
}

fn apply_shared_rows(
    storage: &Storage,
    rows: Vec<crate::records::transfer::Row>,
    snapshot: bool,
) -> Result<(), AppError> {
    let transaction = storage
        .connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    let mut importer = transfer::Import::shared(&transaction, snapshot)?;
    for row in rows {
        importer.insert(&transaction, row)?;
    }
    transfer::Import::finish(&transaction)?;
    transaction.commit().map_err(AppError::database)
}

#[test]
fn native_records_shared_delta_preserves_history_without_changing_chat_certificate() {
    let (_source_dir, mut source) = database();
    let (_target_dir, target) = database();
    let initial = source
        .mutate_record(&create_request("initial", decision()))
        .unwrap();
    apply_shared_rows(&target, shared_rows(&source, None), true).unwrap();
    let certificate = target.silver_certificate().unwrap();
    let base: i64 = source
        .connection
        .query_row("SELECT revision FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    let successor = source
        .mutate_record(&Mutation {
            operation_id: "replacement".into(),
            change: Change::Supersede {
                expected: expected(&initial),
                body: decision(),
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap();
    apply_shared_rows(&target, shared_rows(&source, Some(base)), false).unwrap();
    assert_eq!(target.silver_certificate().unwrap(), certificate);
    assert_eq!(
        target.records(&List::default()).unwrap().records[0].id,
        successor.record_id
    );
    assert!(
        target.record(&initial.record_id, true).unwrap().revisions[0]
            .body
            .details
            .superseded()
    );
    assert_eq!(
        target
            .connection
            .query_row("SELECT count(*) FROM record_operations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn native_records_shared_resolution_delta_carries_both_ends_and_rejects_invalid_links() {
    let (_source_dir, mut source) = database();
    let (_target_dir, target) = database();
    let answer = source
        .mutate_record(&create_request("answer", decision()))
        .unwrap();
    let question = source
        .mutate_record(&create_request(
            "question",
            body(Details::OpenQuestion {
                state: QuestionState::Open,
                answer: None,
            }),
        ))
        .unwrap();
    apply_shared_rows(&target, shared_rows(&source, None), true).unwrap();
    let base: i64 = source
        .connection
        .query_row("SELECT revision FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    let certificate = target.silver_certificate().unwrap();
    source
        .mutate_record(&Mutation {
            operation_id: "link-answer".into(),
            change: Change::Resolve {
                expected: expected(&question),
                answer: "Use Qdrant".into(),
                resolved_by: Some(expected(&answer)),
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap();
    let delta = shared_rows(&source, Some(base));
    apply_shared_rows(&target, delta.clone(), false).unwrap();
    assert_eq!(target.silver_certificate().unwrap(), certificate);
    let linked = target.record(&question.record_id, true).unwrap();
    assert_eq!(linked.current_revision, 2);
    assert_eq!(linked.links.len(), 1);
    assert_eq!(linked.links[0].source_id, answer.record_id);
    let invalid = delta
        .into_iter()
        .map(|row| match row {
            crate::records::transfer::Row::Link { mut link } => {
                std::mem::swap(&mut link.source_id, &mut link.target_id);
                crate::records::transfer::Row::Link { link }
            }
            row => row,
        })
        .collect();
    let error = apply_shared_rows(&target, invalid, false).unwrap_err();
    assert!(
        error.error.message.contains("invalid resolution link"),
        "{error:?}"
    );
    assert_eq!(
        target.record(&question.record_id, true).unwrap().links[0].source_id,
        answer.record_id
    );
}

#[test]
fn native_records_shared_forget_cannot_be_reversed_by_an_older_snapshot() {
    let (_source_dir, mut source) = database();
    let (_target_dir, target) = database();
    let initial = source
        .mutate_record(&create_request("initial", decision()))
        .unwrap();
    let stale = shared_rows(&source, None);
    apply_shared_rows(&target, stale.clone(), true).unwrap();
    source
        .mutate_record(&Mutation {
            operation_id: "forget".into(),
            change: Change::Forget {
                expected: expected(&initial),
            },
        })
        .unwrap();
    apply_shared_rows(&target, shared_rows(&source, None), false).unwrap();
    assert!(apply_shared_rows(&target, stale, true).is_err());
    assert!(target.record(&initial.record_id, true).is_err());
    assert_eq!(
        target
            .connection
            .query_row("SELECT count(*) FROM record_revisions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn native_records_shared_projection_excludes_local_scopes_and_owner_receipts() {
    use crate::records::transfer::Row;
    let (_directory, mut source) = database();
    let mut request = create_request("local", decision());
    if let Change::Create { scope, .. } = &mut request.change {
        *scope = Scope::LocalProject {
            path: "/project".into(),
        };
    }
    source.mutate_record(&request).unwrap();
    assert!(
        shared_rows(&source, None)
            .iter()
            .all(|row| matches!(row, Row::Owner { .. }))
    );
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    #[test]
    fn native_records_generated_choice_sequences_preserve_current_choice_and_retry_identity(
        choices in proptest::collection::vec(0u8..4, 1..10)
    ) {
        let (directory,mut storage)=database();
        let mut current=storage.mutate_record(&create_request("initial",decision())).unwrap();
        let mut ids=std::collections::BTreeSet::from([current.record_id.clone()]);
        for (index,choice) in choices.into_iter().enumerate() {
            let mut next_body=decision(); next_body.statement=format!("Choice {choice}");
            let request=Mutation {operation_id:format!("choice-{index}"),change:Change::Supersede {expected:expected(&current),body:next_body.clone(),evidence:vec![citation("yes","yes")]}};
            let next=storage.mutate_record(&request).unwrap();
            proptest::prop_assert!(ids.insert(next.record_id.clone()));
            drop(storage);
            storage=Storage::open(&directory.path().join("index.sqlite3")).unwrap();
            let retried=storage.mutate_record(&request).unwrap();
            proptest::prop_assert_eq!(&retried.record_id,&next.record_id);
            let active=storage.records(&List::default()).unwrap();
            proptest::prop_assert_eq!(active.records.len(),1);
            proptest::prop_assert_eq!(&active.records[0].revisions[0].body.statement,&next_body.statement);
            proptest::prop_assert_eq!(&active.records[0].id,&next.record_id);
            proptest::prop_assert!(storage.record(&current.record_id,false).unwrap().revisions[0].body.details.superseded());
            current=next;
        }
    }
}
