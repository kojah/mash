use crate::role::Role;
use crate::storage::Storage;
use crate::storage::tests::conversation_with_messages;

#[test]
fn message_turn_updates_use_an_id_lookup_instead_of_scanning_the_chat() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let update = crate::storage::generated::SetMessageTurn::builder()
        .turn_id(Some("turn"))
        .conversation_id("chat")
        .id("entry")
        .build();
    let plan: Vec<String> = storage
        .connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            crate::storage::generated::SetMessageTurn::QUERY
        ))
        .unwrap()
        .query_map(update.as_params(), |row| row.get(3))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(plan.iter().any(|step| step.contains("(id=?)")), "{plan:?}");
}

fn chat(roles: &[(&str, Role, &str)]) -> crate::silver::Conversation {
    let pairs: Vec<(&str, &str)> = roles.iter().map(|(id, _, text)| (*id, *text)).collect();
    let mut chat = conversation_with_messages(&pairs);
    for (message, (_, role, _)) in chat.messages.inline_mut().iter_mut().zip(roles) {
        message.role = role.clone();
    }
    chat
}

/// Each message's turn membership, in ordinal order.
fn memberships(storage: &Storage) -> Vec<(String, Option<String>)> {
    storage
        .connection
        .prepare(
            "SELECT m.id, m.turn_id FROM objects m
              WHERE m.object_role = 'entry' ORDER BY m.ordinal",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn turn_count(storage: &Storage) -> i64 {
    storage
        .connection
        .query_row("SELECT count(*) FROM turns", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn message_turn_lookup_preserves_boundaries_and_checks_search_identity() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let conversation = chat(&[
        ("ask", Role::User, "fix the bug"),
        ("answer", Role::Assistant, "done"),
        ("skill", Role::User, "<skill> injected"),
        ("thanks", Role::User, "thanks"),
    ]);
    storage.replace_conversation(&conversation).unwrap();
    let lookup = |id| {
        super::message_turn(
            &storage.connection,
            conversation.provider,
            &conversation.id,
            id,
        )
        .unwrap()
    };
    let first = lookup("ask").unwrap();
    assert_eq!(lookup("answer").as_deref(), Some(first.as_str()));
    assert_ne!(lookup("thanks").unwrap(), first);
    assert_eq!(lookup("skill"), None);
    assert_eq!(lookup("deleted"), None);
    assert!(
        super::message_turn(
            &storage.connection,
            conversation.provider,
            "wrong-chat",
            "ask",
        )
        .is_err()
    );
    assert!(
        super::message_turn(
            &storage.connection,
            crate::provider::Provider::ClaudeCode,
            &conversation.id,
            "ask",
        )
        .is_err()
    );
}

#[test]
fn indexing_stores_turns_without_reparenting_messages() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let mut first = chat(&[
        ("ask", Role::User, "fix the bug"),
        ("answer", Role::Assistant, "done"),
        ("skill", Role::User, "<skill> injected"),
        ("thanks", Role::User, "thanks"),
    ]);
    storage.replace_conversation(&first).unwrap();
    assert_eq!(turn_count(&storage), 2);
    let membership = memberships(&storage);
    assert_eq!(
        membership
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        ["ask", "answer", "skill", "thanks"]
    );
    let initial_turn = membership[0].1.clone().unwrap();
    assert_eq!(membership[1].1.as_deref(), Some(initial_turn.as_str()));
    assert_eq!(membership[2].1, None);
    assert_ne!(membership[3].1.as_ref().unwrap(), &initial_turn);
    let boundary: String = storage
        .connection
        .query_row(
            "SELECT t.boundary FROM objects m JOIN turns t ON t.id=m.turn_id WHERE m.id='thanks'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(boundary, "user_reply");

    // Dropping the reply removes its turn, but never the surviving messages.
    first.messages.inline_mut().truncate(2);
    storage.replace_conversation(&first).unwrap();
    assert_eq!(turn_count(&storage), 1);
    assert_eq!(
        memberships(&storage),
        [
            ("ask".into(), Some(initial_turn.clone())),
            ("answer".into(), Some(initial_turn)),
        ]
    );
    assert!(
        crate::storage::objects::native_violations(&storage.connection)
            .unwrap()
            .is_empty(),
        "native silver must follow the declared native topology"
    );
    // Searchable counts and listings stay message-only.
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert_eq!(storage.counts().unwrap().conversations, 1);
}

#[test]
fn unchanged_reindexing_leaves_silver_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let thread = chat(&[
        ("ask", Role::User, "question"),
        ("answer", Role::Assistant, "reply"),
    ]);
    storage.replace_conversation(&thread).unwrap();
    let before = storage.silver_certificate().unwrap();
    super::derive(&storage.connection, &thread.id).unwrap();
    assert_eq!(storage.silver_certificate().unwrap(), before);
}

#[test]
fn deleting_derived_turns_preserves_messages_and_session_deletion_cascades() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let session = chat(&[
        ("ask", Role::User, "question"),
        ("answer", Role::Assistant, "reply"),
    ]);
    storage.replace_conversation(&session).unwrap();
    let before = storage.view("ask", 10).unwrap();
    storage.connection.execute("DELETE FROM turns", []).unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert!(memberships(&storage).iter().all(|(_, turn)| turn.is_none()));
    let after = storage.view("ask", 10).unwrap();
    assert_eq!(
        serde_json::to_value(after).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    super::derive(&storage.connection, &session.id).unwrap();
    assert_eq!(turn_count(&storage), 1);
    storage
        .connection
        .execute(
            "DELETE FROM objects WHERE object_role='container' AND id=?1",
            [&session.id],
        )
        .unwrap();
    assert_eq!(turn_count(&storage), 0);
    assert_eq!(storage.counts().unwrap().messages, 0);
}

#[test]
fn agent_topology_accepts_each_provider_and_rejects_invalid_message_parents() {
    for &provider in crate::provider::Provider::ALL {
        let directory = tempfile::tempdir().unwrap();
        let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
        let mut session = chat(&[("ask", Role::User, "question")]);
        session.provider = provider;
        storage.replace_conversation(&session).unwrap();
        storage.commit_writer().unwrap();
        assert_eq!(
            super::super::objects::native_violations(&storage.connection).unwrap(),
            Vec::<String>::new(),
            "{provider}"
        );
        for mutation in [
            "UPDATE objects SET kind='unknown:message' WHERE object_role='entry'",
            "UPDATE objects SET parent_storage_id=NULL WHERE object_role='entry'",
            "UPDATE objects SET parent_storage_id=storage_id WHERE object_role='entry'",
            "UPDATE objects SET source='unknown-provider' WHERE object_role='entry'",
        ] {
            let transaction = storage.connection.unchecked_transaction().unwrap();
            transaction.execute_batch(mutation).unwrap();
            assert_eq!(
                super::super::objects::native_violations(&transaction).unwrap(),
                ["ask"],
                "{provider}: {mutation}"
            );
            transaction.rollback().unwrap();
        }
    }
}
