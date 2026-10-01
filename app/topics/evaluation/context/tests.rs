use super::*;
use rusqlite::params;

fn database(parent: Option<i64>) -> (Connection, Source) {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE messages (id TEXT,conversation_id TEXT,ordinal INTEGER,role TEXT,content TEXT,search_projection TEXT,context_parent INTEGER);").unwrap();
    insert(
        &connection,
        0,
        "user",
        "what do you think of this pipeline?",
        None,
        parent,
    );
    let source = connection.query_row("SELECT id AS message_id,ordinal,content AS canonical_content,false AS stored_projection FROM messages WHERE ordinal=0", [], |row| Source::stored(row, "what do you think of this pipeline?".into())).unwrap();
    (connection, source)
}

fn insert(
    connection: &Connection,
    ordinal: i64,
    role: &str,
    text: &str,
    projection: Option<&str>,
    parent: Option<i64>,
) {
    connection
        .execute(
            "INSERT INTO messages VALUES (?1,'thread',?2,?3,?4,?5,?6)",
            params![
                format!("message-{ordinal}"),
                ordinal,
                role,
                text,
                projection,
                parent
            ],
        )
        .unwrap();
}

fn answer() -> String {
    "Static deadlock detection needs runtime confirmation and careful recall measurement. "
        .repeat(8)
}

#[test]
fn only_generic_deictic_requests_borrow_context() {
    assert!(eligible(
        "what’s your opinion on the current state of this pipeline?"
    ));
    for text in [
        "what do you think of the Redis pipeline?",
        "do that in /tmp",
        "use option 2 for this",
        "what do you think?",
        "what is this unfamiliar thing?",
    ] {
        assert!(!eligible(text), "{text}");
    }
    assert!(!eligible(&"this ".repeat(100)));
}

#[test]
fn empty_tool_envelopes_do_not_end_a_turn_but_unembedded_requests_do() {
    let (connection, source) = database(None);
    insert(&connection, 1, "user", "tool payload", Some(""), None);
    insert(&connection, 2, "assistant", &answer(), None, None);
    insert(
        &connection,
        3,
        "user",
        "next unembedded request",
        None,
        None,
    );
    insert(
        &connection,
        4,
        "assistant",
        &"unrelated subject ".repeat(100),
        None,
        None,
    );
    let Choice::Answer {
        anchor,
        answer: selected,
        complete,
    } = selection::read(&connection, "thread", &source).unwrap()
    else {
        panic!("expected grounded answer");
    };
    assert_eq!(selected.anchor(), Some(("message-2", 2)));
    assert!(complete);
    let contextual = source.with_context(&anchor, &selected, complete).unwrap();
    assert!(contextual.text.contains("deadlock"));
    assert!(!contextual.text.contains("unrelated"));
    assert_eq!(contextual.reference_cost(), 2);
}

#[test]
fn tool_and_planning_text_cannot_supply_subjects() {
    let (connection, source) = database(None);
    insert(&connection, 1, "tool", &answer(), None, None);
    insert(
        &connection,
        2,
        "assistant",
        &format!("I'll investigate. {}", answer()),
        None,
        None,
    );
    assert!(matches!(
        selection::read(&connection, "thread", &source).unwrap(),
        Choice::Skip(Skip::NoAnswer)
    ));
}

#[test]
fn native_sibling_answers_are_ambiguous() {
    let (connection, source) = database(Some(-1));
    insert(&connection, 1, "assistant", &answer(), None, Some(0));
    insert(&connection, 2, "assistant", &answer(), None, Some(0));
    assert!(matches!(
        selection::read(&connection, "thread", &source).unwrap(),
        Choice::Skip(Skip::AmbiguousBranch)
    ));
    connection
        .execute("UPDATE messages SET context_parent=1 WHERE ordinal=2", [])
        .unwrap();
    assert!(matches!(
        selection::read(&connection, "thread", &source).unwrap(),
        Choice::Answer { .. }
    ));
}

#[test]
fn mixed_and_invalid_ancestry_cannot_supply_context() {
    let (connection, source) = database(Some(-1));
    insert(&connection, 1, "assistant", &answer(), None, None);
    assert!(matches!(
        selection::read(&connection, "thread", &source).unwrap(),
        Choice::Skip(Skip::AmbiguousBranch)
    ));
    connection
        .execute("UPDATE messages SET context_parent=1 WHERE ordinal=1", [])
        .unwrap();
    assert!(selection::read(&connection, "thread", &source).is_err());
}

#[test]
fn truncated_windows_and_oversized_answers_are_not_evidence() {
    let (connection, source) = database(None);
    insert(&connection, 1, "assistant", &answer(), None, None);
    for ordinal in 2..=64 {
        insert(&connection, ordinal, "tool", "", None, None);
    }
    assert!(matches!(
        selection::read(&connection, "thread", &source).unwrap(),
        Choice::Skip(Skip::TruncatedWindow)
    ));
    connection
        .execute("DELETE FROM messages WHERE ordinal>1", [])
        .unwrap();
    connection
        .execute(
            "UPDATE messages SET content=?1 WHERE ordinal=1",
            ["x".repeat(65537)],
        )
        .unwrap();
    assert!(matches!(
        selection::read(&connection, "thread", &source).unwrap(),
        Choice::Skip(Skip::NoAnswer)
    ));
}

#[test]
fn prefixes_are_bounded_and_changed_anchors_are_rejected() {
    let (connection, source) = database(None);
    insert(&connection, 1, "assistant", &answer().repeat(8), None, None);
    let Choice::Answer {
        anchor,
        answer: selected,
        complete,
    } = selection::read(&connection, "thread", &source).unwrap()
    else {
        panic!("expected answer");
    };
    assert_eq!(selected.text.chars().count(), 2048);
    assert!(!complete);
    assert!(source.with_context(&anchor, &selected, complete).is_ok());
    connection
        .execute("UPDATE messages SET content='changed' WHERE ordinal=0", [])
        .unwrap();
    let Choice::Answer { anchor, .. } = selection::read(&connection, "thread", &source).unwrap()
    else {
        panic!("expected answer");
    };
    assert!(source.with_context(&anchor, &selected, complete).is_err());
    assert!(
        Source::from("synthetic")
            .with_context(&anchor, &selected, false)
            .is_err()
    );
}

#[test]
fn missing_models_leave_threads_and_canonical_partition_unchanged() {
    let (connection, source) = database(None);
    insert(&connection, 1, "assistant", &answer(), None, None);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("context.sqlite3");
    connection
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let mut thread = super::super::anchored::thread(&[0]);
    thread.user_sources = vec![source];
    let before = thread.clone();
    let mut threads = BTreeMap::from([("thread".into(), thread)]);
    assert!(enrich(&path, &mut threads, None).is_err());
    assert_eq!(threads["thread"].source_id, before.source_id);
    assert_eq!(
        threads["thread"].user_messages[0].vector,
        before.user_messages[0].vector
    );
    assert_eq!(
        threads["thread"].user_messages[0].words,
        before.user_messages[0].words
    );
    assert_eq!(
        threads["thread"].user_sources[0].text,
        before.user_sources[0].text
    );
    assert_eq!(threads["thread"].user_sources[0].reference_cost(), 1);
}
