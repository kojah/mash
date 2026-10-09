use super::*;
use serde_json::json;

fn id(value: &str) -> Id {
    Id::try_from(value.to_owned()).unwrap()
}

fn batch(source: &str, name: &str, revision: u64, changes: serde_json::Value) -> Batch {
    let mut envelope = json!({"schema_version":2,"source_id":source,
        "batch_id":name,"expected_revision":revision,"changes":[]});
    envelope["changes"] = changes;
    Batch::decode(&serde_json::to_vec(&envelope).unwrap()).unwrap()
}

fn container(id: &str) -> serde_json::Value {
    json!({"op":"upsert-object","id":id,"role":"container","kind":"chat:channel"})
}

fn entry(id: &str, parent: &str, content: &str) -> serde_json::Value {
    json!({"op":"upsert-object","id":id,"role":"entry","kind":"chat:message",
        "parent":parent,"content":content})
}

fn document(id: &str, content: &str) -> serde_json::Value {
    json!({"op":"upsert-object","id":id,"role":"document","kind":"notes:note","content":content})
}

fn initial(source: &str) -> Batch {
    let mut message = entry("message", "chat", "A human's message");
    message["author"] = json!({"id":"alice","kind":"human"});
    message["metadata"] = json!({"reaction":"👍"});
    let mut note = document("note", "Canonical note");
    note["attachments"] = json!([{"id":"file","name":"diagram.png","media_type":"image/png",
        "url":"https://example.com/file"}]);
    let mut chat = container("chat");
    chat["title"] = json!("Engineering");
    // The entry precedes its parent: the edge is checked when the batch commits.
    batch(source, "first", 0, json!([message, chat, note]))
}

fn content(store: &Store, source: &str, object: &str) -> Option<String> {
    store
        .object(&id(source), &id(object))
        .unwrap()
        .and_then(|object| object.content)
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
        .object(&id("source"), &id("note"))
        .unwrap()
        .unwrap();
    assert_eq!(note.content.as_deref(), Some("Canonical note"));
    assert_eq!(note.attachments.len(), 1);
    let second = batch(
        "source",
        "second",
        1,
        json!([document("note", "Edited note")]),
    );
    let next = reopened.apply(&second).unwrap();
    assert_eq!(next.source_revision, 2);
    assert!(next.silver_revision > receipt.silver_revision);
    assert_eq!(reopened.apply(&first).unwrap(), receipt);
    assert_eq!(
        content(&reopened, "source", "note").as_deref(),
        Some("Edited note")
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
            json!([document("note", "Should not overwrite")]),
        );
        assert_eq!(store.apply(&changed), Err(error));
    }
    assert_eq!(
        content(&store, "source", "note").as_deref(),
        Some("Canonical note")
    );
}

#[test]
fn deleting_a_container_deletes_its_subtree_only_within_its_source() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    store.apply(&initial("first-source")).unwrap();
    store.apply(&initial("second-source")).unwrap();
    let deletion = batch(
        "first-source",
        "delete",
        1,
        json!([{"op":"delete-object","id":"chat"}]),
    );
    store.apply(&deletion).unwrap();
    for (source, object, present) in [
        ("first-source", "chat", false),
        ("first-source", "message", false),
        ("first-source", "note", true),
        ("second-source", "message", true),
    ] {
        assert_eq!(
            store.object(&id(source), &id(object)).unwrap().is_some(),
            present,
            "{source} {object}"
        );
    }
}

#[test]
fn stored_graph_violations_roll_back_entire_batches() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    let mut parent = entry("parent", "a", "Parent");
    parent["reply_to"] = json!("child");
    // Window-excluded reply targets may remain unresolved.
    store
        .apply(&batch(
            "source",
            "first",
            0,
            json!([container("a"), parent, document("doc", "Doc")]),
        ))
        .unwrap();
    let mut child = entry("child", "a", "Child");
    child["reply_to"] = json!("parent");
    let mut segment = container("segment");
    segment["role"] = json!("segment");
    let violations = [
        // A reply cycle through stored objects.
        json!([child, document("new-note", "Must roll back")]),
        // A parent that does not exist.
        json!([entry("orphan", "missing", "Orphan")]),
        // An entry whose only ancestor is a segment, not a container.
        json!([segment, entry("loose", "segment", "Loose")]),
        // A stored document cannot gain children.
        json!([entry("inside", "doc", "Inside")]),
        // An existing parent cannot become a document.
        json!([document("a", "Now a note")]),
    ];
    for changes in violations {
        let rejected = batch("source", "second", 1, changes.clone());
        assert_eq!(
            store.apply(&rejected),
            Err(Error::InvalidReferences),
            "{changes}"
        );
        assert!(
            store
                .object(&id("source"), &id("new-note"))
                .unwrap()
                .is_none()
        );
    }
    let valid = batch(
        "source",
        "second",
        1,
        json!([document("new-note", "Retry succeeds")]),
    );
    assert_eq!(store.apply(&valid).unwrap().source_revision, 2);
}

#[test]
fn failure_to_record_receipt_rolls_back_data_and_ordering() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    store.connection.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON connector_receipts BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert_eq!(store.apply(&initial("source")), Err(Error::Database));
    assert!(store.object(&id("source"), &id("chat")).unwrap().is_none());
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
    let note = store.object(&id("source"), &id("note")).unwrap().unwrap();
    assert_eq!(note.attachments.len(), 1);
    let new_identity = directory.path().join("new-identity.sqlite3");
    crate::storage::export::restore(&archive, &new_identity, false).unwrap();
    let mut fresh = Store::open(&new_identity).unwrap();
    assert_eq!(fresh.apply(&first).unwrap().source_revision, 1);
    assert!(
        fresh
            .object(&id("source"), &id("message"))
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
