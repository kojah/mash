use super::*;
use crate::records::recall::{Request, Status};
use crate::redaction::Policy;

fn request() -> Request {
    Request {
        scope: Scope::Repository {
            identity: "repo".into(),
        },
        query: None,
        file: None,
        include_personal: false,
        include_review: false,
        limit: None,
        byte_budget: None,
    }
}

#[test]
fn native_records_recall_ranks_without_confirming_and_labels_review() {
    let (_directory, mut storage) = database();
    let first = storage
        .mutate_record(&create_request("first", decision()))
        .unwrap();
    let mut other = decision();
    other.statement = "Use SQLite".into();
    let second = storage
        .mutate_record(&create_request("second", other))
        .unwrap();
    storage
        .mutate_record(&Mutation {
            operation_id: "confirmation".into(),
            change: Change::Confirm {
                expected: expected(&first),
                evidence: vec![citation("again", "Still choose Qdrant.")],
            },
        })
        .unwrap();
    let before = storage
        .connection
        .query_row("SELECT revision FROM shared_publication", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    let recalled = storage.recall_records(&request(), Policy::ON).unwrap();
    assert_eq!(recalled.records[0].id, first.record_id);
    assert_eq!(recalled.records[0].confirmations, 1);
    assert!(recalled.records[0].last_confirmed_at.is_some());
    assert_eq!(
        storage
            .recall_records(&request(), Policy::ON)
            .unwrap()
            .records[0]
            .confirmations,
        1
    );
    assert_eq!(
        storage
            .connection
            .query_row("SELECT revision FROM shared_publication", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        before
    );
    let mut query = request();
    query.query = Some("SQLite".into());
    assert_eq!(
        storage.recall_records(&query, Policy::ON).unwrap().records[0].id,
        second.record_id
    );
    storage.connection.execute("UPDATE record_revisions SET body=json_set(body,'$.review','candidate') WHERE record_id=?1", [&second.record_id]).unwrap();
    let default = storage.recall_records(&request(), Policy::ON).unwrap();
    assert_eq!(
        default
            .records
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        [Status::Current, Status::Candidate],
        "candidates are recalled by default, labeled, after current records"
    );
    let mut review = request();
    review.include_review = true;
    assert_eq!(
        storage.recall_records(&review, Policy::ON).unwrap().records[1].status,
        Status::Candidate
    );
    storage
        .connection
        .execute("UPDATE messages SET role='assistant' WHERE id='yes'", [])
        .unwrap();
    assert!(
        storage
            .recall_records(&request(), Policy::ON)
            .unwrap()
            .records
            .is_empty()
    );
    assert!(
        storage
            .recall_records(&review, Policy::ON)
            .unwrap()
            .records
            .iter()
            .all(|item| item.status == Status::StaleEvidence)
    );
}

#[test]
fn native_records_recall_scopes_files_and_packet_budget_are_bounded() {
    let (_directory, mut storage) = database();
    for (name, text) in [
        ("exact", "Edit `src/main.rs`"),
        ("suffix", "Edit src/main.rs.bak"),
        ("prefix", "Edit other/src/main.rs"),
    ] {
        let mut body = decision();
        body.statement = text.into();
        storage.mutate_record(&create_request(name, body)).unwrap();
    }
    let mut input = request();
    input.file = Some("src/main.rs".into());
    let result = storage.recall_records(&input, Policy::ON).unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].body.statement, "Edit `src/main.rs`");
    input.scope = Scope::Repository {
        identity: "other-repo".into(),
    };
    assert!(
        storage
            .recall_records(&input, Policy::ON)
            .unwrap()
            .records
            .is_empty()
    );
    input = request();
    input.byte_budget = Some(1024);
    let result = storage.recall_records(&input, Policy::ON).unwrap();
    assert!(result.truncated);
    assert!(encode(&result).unwrap().len() <= 1024);
    for file in [
        "../src/main.rs",
        "/src/main.rs",
        "src/../main.rs",
        "src\\main.rs",
    ] {
        input.file = Some(file.into());
        assert!(storage.recall_records(&input, Policy::ON).is_err());
    }
}

#[test]
fn native_records_recall_discloses_candidate_limit_and_does_not_leak_credentials() {
    let (_directory, mut storage) = database();
    for index in 0..257 {
        let mut value = decision();
        value.statement = format!("Choice {index}");
        storage
            .mutate_record(&create_request(&format!("r{index}"), value))
            .unwrap();
    }
    let result = storage.recall_records(&request(), Policy::ON).unwrap();
    assert_eq!(result.scanned, 256);
    assert!(result.scan_limited && result.truncated);
    assert!(result.records.len() <= 8);
    let mut secret = decision();
    secret.statement = "Use token ghp_1234567890abcdefghijklmnopqrstuvwxyz".into();
    storage
        .mutate_record(&create_request("secret", secret))
        .unwrap();
    let mut input = request();
    input.query = Some("ghp_1234567890abcdefghijklmnopqrstuvwxyz".into());
    assert!(
        storage
            .recall_records(&input, Policy::ON)
            .unwrap()
            .records
            .is_empty()
    );
}
