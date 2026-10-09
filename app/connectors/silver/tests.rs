use super::*;
use serde_json::json;

fn id(value: &str) -> Id {
    Id::try_from(value.to_owned()).unwrap()
}

fn batch(source: &str, name: &str, revision: u64, changes: serde_json::Value) -> Batch {
    let mut envelope = json!({"schema_version":1,"source_id":source,
        "batch_id":name,"expected_revision":revision,"changes":[]});
    envelope["changes"] = changes;
    Batch::decode(&serde_json::to_vec(&envelope).unwrap()).unwrap()
}

fn initial(source: &str) -> Batch {
    batch(
        source,
        "first",
        0,
        json!([
            {"op":"upsert-message","id":"message","chat_id":"chat","content":"A human's message",
             "author":{"id":"alice","kind":"human"},"metadata":{"reaction":"👍"}},
            {"op":"upsert-chat","id":"chat","title":"Engineering","metadata":{"workspace":"work"}},
            {"op":"upsert-document","id":"note","content":"Canonical note","metadata":{"tags":["storage"]},
             "attachments":[{"id":"file","name":"diagram.png","media_type":"image/png","url":"https://example.com/file"}]}
        ]),
    )
}

#[test]
fn committed_records_and_receipt_survive_lost_response_reopen_and_later_batches() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("silver.sqlite3");
    let first = initial("source");
    let receipt = Store::open(&path).unwrap().apply(&first).unwrap();
    assert_eq!(receipt.source_revision, 1);
    assert!(!receipt.gold_ready);
    let mut reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.apply(&first).unwrap(), receipt);
    let note = reopened
        .document(&id("source"), &id("note"))
        .unwrap()
        .unwrap();
    assert_eq!(note.content, "Canonical note");
    assert_eq!(note.attachments.len(), 1);
    let second = batch(
        "source",
        "second",
        1,
        json!([
            {"op":"upsert-document","id":"note","content":"Edited note"}
        ]),
    );
    let next = reopened.apply(&second).unwrap();
    assert_eq!(next.source_revision, 2);
    assert!(next.silver_revision > receipt.silver_revision);
    assert_eq!(reopened.apply(&first).unwrap(), receipt);
    assert_eq!(
        reopened
            .document(&id("source"), &id("note"))
            .unwrap()
            .unwrap()
            .content,
        "Edited note"
    );
}

#[test]
fn changed_batch_identity_and_stale_source_revision_never_mutate_records() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    store.apply(&initial("source")).unwrap();
    for (name, expected, error) in [
        ("first", 0, Error::BatchConflict),
        ("another", 0, Error::RevisionConflict),
    ] {
        let changed = batch(
            "source",
            name,
            expected,
            json!([
                {"op":"upsert-document","id":"note","content":"Should not overwrite"}
            ]),
        );
        assert_eq!(store.apply(&changed), Err(error));
    }
    assert_eq!(
        store
            .document(&id("source"), &id("note"))
            .unwrap()
            .unwrap()
            .content,
        "Canonical note"
    );
}

#[test]
fn source_namespaces_and_chat_deletion_preserve_other_sources_and_documents() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    store.apply(&initial("first-source")).unwrap();
    store.apply(&initial("second-source")).unwrap();
    let deletion = batch(
        "first-source",
        "delete",
        1,
        json!([{"op":"delete-chat","id":"chat"}]),
    );
    store.apply(&deletion).unwrap();
    assert!(
        store
            .chat(&id("first-source"), &id("chat"))
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .message(&id("first-source"), &id("message"))
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .document(&id("first-source"), &id("note"))
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .message(&id("second-source"), &id("message"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn stored_cycles_and_cross_chat_links_rollback_entire_batches() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    let first = batch(
        "source",
        "first",
        0,
        json!([
            {"op":"upsert-chat","id":"a"}, {"op":"upsert-chat","id":"b"},
            {"op":"upsert-message","id":"parent","chat_id":"a","content":"Parent","reply_to":"child"}
        ]),
    );
    store.apply(&first).unwrap(); // Window-excluded parents may remain unresolved.
    let cyclic = batch(
        "source",
        "second",
        1,
        json!([
            {"op":"upsert-message","id":"child","chat_id":"a","content":"Child","reply_to":"parent"},
            {"op":"upsert-document","id":"new-note","content":"Must roll back"}
        ]),
    );
    assert_eq!(store.apply(&cyclic), Err(Error::InvalidReferences));
    assert!(
        store
            .document(&id("source"), &id("new-note"))
            .unwrap()
            .is_none()
    );
    // A memoized ancestor from chat A cannot validate another child in chat B.
    let cross_chat = batch(
        "source",
        "second",
        1,
        json!([
            {"op":"upsert-message","id":"x","chat_id":"a","content":"X","reply_to":"parent"},
            {"op":"upsert-message","id":"y","chat_id":"b","content":"Y","reply_to":"parent"}
        ]),
    );
    assert_eq!(store.apply(&cross_chat), Err(Error::InvalidReferences));
    assert!(store.message(&id("source"), &id("x")).unwrap().is_none());
    let valid = batch(
        "source",
        "second",
        1,
        json!([{"op":"upsert-document","id":"new-note","content":"Retry succeeds"}]),
    );
    assert_eq!(store.apply(&valid).unwrap().source_revision, 2);
}

#[test]
fn failure_to_record_receipt_rolls_back_data_and_ordering() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    store.connection.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON connector_receipts BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert_eq!(store.apply(&initial("source")), Err(Error::Database));
    assert!(store.chat(&id("source"), &id("chat")).unwrap().is_none());
    store
        .connection
        .execute_batch("DROP TRIGGER fail_receipt;")
        .unwrap();
    assert_eq!(store.apply(&initial("source")).unwrap().source_revision, 1);
}

#[test]
fn canonical_exports_preserve_external_records_and_upgrade_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.sqlite3");
    let archive = directory.path().join("snapshot.jsonl");
    let first = initial("source");
    let receipt = Store::open(&source).unwrap().apply(&first).unwrap();
    crate::storage::export::write(&source, &archive).unwrap();
    let restored = directory.path().join("restored.sqlite3");
    crate::storage::export::restore(&archive, &restored, true).unwrap();
    let mut store = Store::open(&restored).unwrap();
    assert_eq!(store.apply(&first).unwrap(), receipt);
    assert_eq!(
        store
            .document(&id("source"), &id("note"))
            .unwrap()
            .unwrap()
            .attachments
            .len(),
        1
    );
    let new_identity = directory.path().join("new-identity.sqlite3");
    crate::storage::export::restore(&archive, &new_identity, false).unwrap();
    let mut fresh = Store::open(&new_identity).unwrap();
    assert_eq!(fresh.apply(&first).unwrap().source_revision, 1);
    assert!(
        fresh
            .message(&id("source"), &id("message"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn unchanged_replacements_advance_ordering_without_invalidating_silver_again() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    let first = initial("source");
    let receipt = store.apply(&first).unwrap();
    let mut unchanged = first.clone();
    unchanged.batch_id = id("second");
    unchanged.expected_revision = 1;
    let next = store.apply(&unchanged).unwrap();
    assert_eq!(next.source_revision, 2);
    assert_eq!(next.silver_revision, receipt.silver_revision);
}
