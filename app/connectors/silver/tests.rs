use super::*;
use serde_json::json;
mod properties;

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
        "parent":parent,"content":content,
        "author":{"id":"alice","kind":"human"}})
}

fn document(id: &str, content: &str) -> serde_json::Value {
    json!({"op":"upsert-object","id":id,"role":"document","kind":"notes:note","content":content})
}

fn initial(source: &str) -> Batch {
    let mut message = entry("message", "chat", "A human's message");
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
    assert!(!receipt.views_ready);
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
    assert!(!directory.path().join("restored.views.sqlite3").exists());
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

fn declared() -> serde_json::Value {
    json!({"op":"declare-schema","kinds":[
        {"kind":"chat:channel","role":"container","root":true},
        {"kind":"chat:archive","role":"container","root":true},
        {"kind":"chat:message","role":"entry","parents":["chat:channel"]},
        {"kind":"notes:note","role":"document","root":true}
    ]})
}

#[test]
fn declared_schemas_hold_for_stored_objects_and_later_batches() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("silver.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let mut thread = container("thread");
    thread["kind"] = json!("chat:thread");
    store
        .apply(&batch(
            "source",
            "first",
            0,
            json!([container("chat"), entry("message", "chat", "Hello"), thread]),
        ))
        .unwrap();
    // The stored chat:thread container is undeclared: the schema is rejected
    // and nothing in the batch applies.
    let rejected = batch(
        "source",
        "second",
        1,
        json!([declared(), document("note", "Must roll back")]),
    );
    assert_eq!(store.apply(&rejected), Err(Error::SchemaViolation));
    assert!(store.object(&id("source"), &id("note")).unwrap().is_none());
    let accepted = batch(
        "source",
        "second",
        1,
        json!([declared(), {"op":"delete-object","id":"thread"}]),
    );
    assert_eq!(store.apply(&accepted).unwrap().source_revision, 2);
    // Later batches without a declaration are checked against the stored one.
    let mut reaction = entry("reaction", "chat", "👍");
    reaction["kind"] = json!("chat:reaction");
    assert_eq!(
        store.apply(&batch("source", "third", 2, json!([reaction]))),
        Err(Error::SchemaViolation)
    );
    // A parent's new kind must still admit its existing children.
    let mut renamed = container("chat");
    renamed["kind"] = json!("chat:archive");
    assert_eq!(
        store.apply(&batch("source", "third", 2, json!([renamed]))),
        Err(Error::SchemaViolation)
    );
    let archive = directory.path().join("archive.jsonl");
    crate::storage::export::write(&path, &archive).unwrap();
    let restored = directory.path().join("restored.sqlite3");
    crate::storage::export::restore(&archive, &restored, true).unwrap();
    let mut restored = Store::open(&restored).unwrap();
    let mut reaction = entry("reaction", "chat", "👍");
    reaction["kind"] = json!("chat:reaction");
    assert_eq!(
        restored.apply(&batch("source", "third", 2, json!([reaction]))),
        Err(Error::SchemaViolation)
    );
}

fn labelled(
    name: &str,
    revision: u64,
    trigger: &str,
    modes: &[&str],
) -> Result<Batch, crate::connectors::Error> {
    let envelope = json!({"schema_version":2,"source_id":"source","batch_id":name,
        "expected_revision":revision,"changes":[document("note", name)],
        "trigger":trigger,"modes":modes});
    Batch::decode(&serde_json::to_vec(&envelope).unwrap())
}

#[test]
fn batches_must_come_from_a_declared_mode() {
    use crate::connectors::Error;
    // A live connector backfills with snapshots; a polled one never sends live.
    assert!(labelled("a", 0, "complete-snapshot", &["live"]).is_ok());
    assert!(labelled("a", 0, "change", &["snapshot"]).is_ok());
    assert_eq!(
        labelled("a", 0, "live", &["snapshot"]).unwrap_err(),
        Error::UndeclaredMode
    );
    assert_eq!(
        labelled("a", 0, "snapshot", &["live", "live"]).unwrap_err(),
        Error::UndeclaredMode
    );
    // Clients that declare nothing are not checked.
    assert!(labelled("a", 0, "live", &[]).is_ok());
}

#[test]
fn complete_snapshot_deletes_containers_it_no_longer_captures() {
    use crate::connectors::Trigger;

    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();

    let mut first = batch(
        "source",
        "scope-1",
        0,
        json!([
            container("kept"),
            container("dropped"),
            entry("kept-message", "kept", "kept text"),
            entry("dropped-message", "dropped", "dropped text")
        ]),
    );
    first.trigger = Some(Trigger::CompleteSnapshot);
    store.apply(&first).unwrap();
    assert!(content(&store, "source", "dropped-message").is_some());

    let mut second = batch("source", "scope-2", 1, json!([container("kept")]));
    second.trigger = Some(Trigger::CompleteSnapshot);
    store.apply(&second).unwrap();
    assert!(
        store
            .object(&id("source"), &id("dropped"))
            .unwrap()
            .is_none()
    );
    assert!(content(&store, "source", "dropped-message").is_none());
    assert!(content(&store, "source", "kept-message").is_some());
}

#[test]
fn child_lookups_use_the_parent_index_instead_of_walking_the_source() {
    // Grouping and schema checks run this once per object; walking every
    // object of the source each time made large deliveries quadratic.
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    let children = sql::ConnectorChildren::builder()
        .source_id("slack:T1:U1")
        .parent_id(Some("C1"))
        .build();
    let plan: Vec<String> = store
        .connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            sql::ConnectorChildren::QUERY
        ))
        .unwrap()
        .query_map(children.as_params(), |row| row.get(3))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(
        plan.iter().any(|step| step.contains("objects_parent")),
        "{plan:?}"
    );
}

#[test]
fn moved_entries_take_their_replies_and_resent_containers_keep_theirs() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("silver.sqlite3")).unwrap();
    let source = "chat:team";
    let grouped = |store: &Store, id: &str| -> String {
        store
            .connection
            .query_row(
                "SELECT conversation_id FROM objects WHERE origin='connector' AND source=?1 AND id=?2",
                [source, id],
                |row| row.get(0),
            )
            .unwrap()
    };
    store
        .apply(&batch(
            source,
            "first",
            0,
            json!([
                container("a"),
                container("b"),
                entry("thread", "a", "a thread"),
                entry("reply", "thread", "a reply")
            ]),
        ))
        .unwrap();
    assert_eq!(grouped(&store, "reply"), "a");
    // Re-sending a container unchanged regroups nothing below it.
    store
        .apply(&batch(source, "again", 1, json!([container("a")])))
        .unwrap();
    assert_eq!(grouped(&store, "thread"), "a");
    assert_eq!(grouped(&store, "reply"), "a");
    // Moving the thread to another container takes its reply along.
    store
        .apply(&batch(
            source,
            "move",
            2,
            json!([entry("thread", "b", "a thread")]),
        ))
        .unwrap();
    assert_eq!(grouped(&store, "thread"), "b");
    assert_eq!(grouped(&store, "reply"), "b");
}
