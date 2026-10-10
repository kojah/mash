use crate::role::Role;
use crate::storage::Storage;
use crate::storage::tests::conversation_with_messages;

#[test]
fn entry_parent_updates_use_a_native_id_lookup_instead_of_scanning_the_chat() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let update = crate::storage::generated::SetEntryParent::builder()
        .parent_id(Some("turn"))
        .parent_storage_id(Some(1))
        .conversation_id("chat")
        .id("entry")
        .build();
    let plan: Vec<String> = storage
        .connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            crate::storage::generated::SetEntryParent::QUERY
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

/// Each message's parent kind and ID, in ordinal order.
fn parents(storage: &Storage) -> Vec<(String, String)> {
    storage
        .connection
        .prepare(
            "SELECT m.id, p.object_role FROM objects m
               JOIN objects p ON p.storage_id = m.parent_storage_id
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
        .query_row(
            "SELECT count(*) FROM objects WHERE object_role = 'segment'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn indexing_stores_turns_between_chats_and_their_messages() {
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
    let segment = |id: &str| (id.to_owned(), "segment".to_owned());
    assert_eq!(
        parents(&storage),
        [
            segment("ask"),
            segment("answer"),
            // Injected instructions belong to no turn.
            ("skill".to_owned(), "container".to_owned()),
            segment("thanks"),
        ]
    );
    let (kind, metadata): (String, String) = storage
        .connection
        .query_row(
            "SELECT t.kind, t.metadata FROM objects m JOIN objects t ON t.storage_id = m.parent_storage_id
              WHERE m.id = 'thanks'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, "codex:turn");
    assert!(
        metadata.contains(r#""boundary":"user_reply""#),
        "{metadata}"
    );

    // Dropping the reply removes its turn, but never the surviving messages.
    first.messages.inline_mut().truncate(2);
    storage.replace_conversation(&first).unwrap();
    assert_eq!(turn_count(&storage), 1);
    assert_eq!(parents(&storage), [segment("ask"), segment("answer")]);
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
fn every_provider_declares_a_valid_native_topology() {
    for &provider in crate::provider::Provider::ALL {
        crate::source::native_schema(provider).validate().unwrap();
    }
}
