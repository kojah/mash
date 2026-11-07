use super::*;
use crate::records::{DecisionState, FactState, PreferenceState};

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
    let failed = create_request("interrupted", decision());
    assert!(storage.mutate_record(&failed).is_err());
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
        assert!(record.revisions[0].evidence_current);
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
