use super::*;
use crate::records::extraction::{Draft, Inspect, Request};

#[test]
fn native_records_extraction_archive_preserves_checkpoints_and_owner_retry_receipts() {
    let (directory, mut storage, request, inspect) = setup();
    storage
        .connection
        .execute("UPDATE chats SET source_key=id", [])
        .unwrap();
    let receipt = storage.extract_records(&request).unwrap();
    let archive = directory.path().join("extracted.jsonl");
    crate::storage::export::write(&directory.path().join("index.sqlite3"), &archive).unwrap();
    for keep in [false, true] {
        let path = directory.path().join(if keep {
            "restored.sqlite3"
        } else {
            "copied.sqlite3"
        });
        crate::storage::export::restore(&archive, &path, keep).unwrap();
        let mut restored = Storage::open(&path).unwrap();
        assert!(restored.extraction_checkpoint(&inspect).unwrap().up_to_date);
        let saved: i64 = restored
            .connection
            .query_row("SELECT count(*) FROM record_extractions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(saved, i64::from(keep));
        let record_id: String = restored
            .connection
            .query_row("SELECT id FROM records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(record_id == receipt.records[0].record_id, keep);
        assert_eq!(
            restored.record(&record_id, true).unwrap().revisions[0]
                .body
                .review,
            Review::Candidate
        );
        if keep {
            assert_eq!(
                encode(&restored.extract_records(&request).unwrap()).unwrap(),
                encode(&receipt).unwrap()
            );
            restored
                .mutate_record(&Mutation {
                    operation_id: "forget-extracted".into(),
                    change: Change::Forget {
                        expected: expected(&receipt.records[0]),
                    },
                })
                .unwrap();
            restored.extract_records(&request).unwrap();
            assert!(restored.record(&record_id, true).is_err());
        }
    }
    let mut shared = Vec::new();
    transfer::visit_shared(
        &storage.connection,
        None,
        crate::redaction::Policy::ON,
        |row| {
            shared.push(row);
            Ok(())
        },
    )
    .unwrap();
    assert!(!shared.iter().any(|row| matches!(
        row,
        records::transfer::Row::ExtractionOperation { .. }
            | records::transfer::Row::Checkpoint { .. }
    )));
}

fn setup() -> (tempfile::TempDir, Storage, Request, Inspect) {
    let (directory, storage) = database();
    let fingerprint = blake3::hash(b"reviewed chat version").to_hex().to_string();
    storage
        .connection
        .execute(
            "UPDATE chats SET source_fingerprint=?1 WHERE id='chat'",
            [&fingerprint],
        )
        .unwrap();
    let scope = Scope::Repository {
        identity: "repo".into(),
    };
    let mut body = decision();
    body.review = Review::Candidate;
    let request = Request {
        operation_id: "extract-chat".into(),
        scope: scope.clone(),
        chat_id: "chat".into(),
        source_fingerprint: fingerprint,
        records: vec![Draft {
            body,
            evidence: vec![citation("proposal", "Choose Qdrant")],
            on_duplicate: records::DuplicatePolicy::Report,
        }],
    };
    let inspect = Inspect {
        scope,
        chat_id: "chat".into(),
    };
    (directory, storage, request, inspect)
}

#[test]
fn native_records_extraction_retry_preserves_receipt_after_source_changes() {
    let (directory, mut storage, request, inspect) = setup();
    assert!(!storage.extraction_checkpoint(&inspect).unwrap().up_to_date);
    let receipt = storage.extract_records(&request).unwrap();
    assert!(storage.extraction_checkpoint(&inspect).unwrap().up_to_date);
    let record = storage.record(&receipt.records[0].record_id, true).unwrap();
    assert_eq!(record.revisions[0].body.review, Review::Candidate);
    assert_eq!(record.revisions[0].confirmations, 0);
    storage
        .connection
        .execute(
            "UPDATE chats SET source_fingerprint=?1 WHERE id='chat'",
            ["b".repeat(64)],
        )
        .unwrap();
    drop(storage);
    let mut storage = Storage::open(&directory.path().join("index.sqlite3")).unwrap();
    assert!(!storage.extraction_checkpoint(&inspect).unwrap().up_to_date);
    assert_eq!(
        encode(&storage.extract_records(&request).unwrap()).unwrap(),
        encode(&receipt).unwrap()
    );
    let mut changed = request.clone();
    changed.operation_id = "new-attempt".into();
    assert!(
        storage
            .extract_records(&changed)
            .unwrap_err()
            .error
            .message
            .contains("chat changed")
    );
    changed.operation_id = request.operation_id;
    changed.records.clear();
    assert!(
        storage
            .extract_records(&changed)
            .unwrap_err()
            .error
            .message
            .contains("different request")
    );
}

#[test]
fn native_records_extraction_failure_rolls_back_records_checkpoint_and_publication() {
    let (directory, mut storage, mut request, inspect) = setup();
    let publication = |storage: &Storage| {
        storage
            .connection
            .query_row("SELECT revision FROM shared_publication", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    let before = publication(&storage);
    let mut invalid = request.records[0].clone();
    invalid.body.statement = "Invalid second draft".into();
    invalid.evidence[0].message_id = "other-message".into();
    request.records.push(invalid);
    assert!(storage.extract_records(&request).is_err());
    assert!(!storage.extraction_checkpoint(&inspect).unwrap().up_to_date);
    assert_eq!(publication(&storage), before);
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM records", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    request.records.pop();
    storage.connection.execute_batch("CREATE TEMP TRIGGER interrupt_extraction BEFORE INSERT ON record_extractions BEGIN SELECT RAISE(ABORT,'interrupted extraction receipt'); END;").unwrap();
    assert!(
        storage
            .extract_records(&request)
            .unwrap_err()
            .error
            .message
            .contains("interrupted extraction receipt")
    );
    drop(storage);
    let mut storage = Storage::open(&directory.path().join("index.sqlite3")).unwrap();
    assert!(!storage.extraction_checkpoint(&inspect).unwrap().up_to_date);
    assert_eq!(publication(&storage), before);
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM records", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM record_extractions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    storage.extract_records(&request).unwrap();
    assert!(storage.extraction_checkpoint(&inspect).unwrap().up_to_date);
}

#[test]
fn native_records_extraction_empty_review_and_candidate_only_guards() {
    let (_directory, mut storage, mut request, inspect) = setup();
    request.records[0].body.review = Review::Established;
    assert!(
        storage
            .extract_records(&request)
            .unwrap_err()
            .error
            .message
            .contains("candidates only")
    );
    assert!(!storage.extraction_checkpoint(&inspect).unwrap().up_to_date);
    request.records.clear();
    assert!(
        storage
            .extract_records(&request)
            .unwrap()
            .records
            .is_empty()
    );
    assert!(storage.extraction_checkpoint(&inspect).unwrap().up_to_date);
    let wrong_scope = Inspect {
        scope: Scope::Repository {
            identity: "other-repo".into(),
        },
        chat_id: "chat".into(),
    };
    assert!(storage.extraction_checkpoint(&wrong_scope).is_err());
    storage
        .connection
        .execute("UPDATE chats SET source_fingerprint='' WHERE id='chat'", [])
        .unwrap();
    assert!(
        storage
            .extraction_checkpoint(&inspect)
            .unwrap_err()
            .error
            .message
            .contains("reindex")
    );
}
