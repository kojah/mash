//! Exercise reconciliation and persistence against independently assembled rows.
use super::*;
use crate::message_context::MessageContext;
use proptest::prelude::*;

type Row = (String, i64, String, String);

fn rows(storage: &Storage) -> Vec<Row> {
    storage
        .connection
        .prepare("SELECT id,ordinal,content,context_parent FROM messages ORDER BY ordinal")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn check(storage: &Storage, expected: &[Row]) {
    assert_eq!(rows(storage), expected);
    let invalid: i64 = storage
        .connection
        .query_row(
            "SELECT count(*) FROM messages m LEFT JOIN turns t ON t.id=m.turn_id
         WHERE t.id IS NULL OR t.chat_id <> m.chat_id",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        invalid, 0,
        "every dialogue message has a turn in its own chat"
    );
    let dangling: i64 = storage
        .connection
        .query_row(
            "SELECT count(*) FROM turns t WHERE NOT EXISTS
         (SELECT 1 FROM messages m WHERE m.turn_id=t.id)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(dangling, 0, "removed branches leave no stale turns");
    let foreign_keys: i64 = storage
        .connection
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(foreign_keys, 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_history_reconciliation_preserves_identity_through_reopen_and_archive(
        snapshots in prop::collection::vec(
            prop::collection::btree_map(0u8..8, (any::<u8>(), 0u8..8), 0..8), 2..8
        )
    ) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silver.sqlite3");
        let mut storage = Storage::open_writer(&path).unwrap();
        let mut expected = Vec::new();
        for snapshot in snapshots {
            // Stable IDs come from map keys; priorities independently choose
            // display order. Content changes, insertions and removals do not
            // change the parent ID of retained replies.
            let mut ordered: Vec<_> = snapshot.into_iter().collect();
            ordered.sort_by_key(|(id, (priority, _))| (*priority, *id));
            expected = vec![("root".into(), 0, "request".into(), String::new())];
            for (position, (id, (_, version))) in ordered.iter().enumerate() {
                expected.push((format!("reply-{id}"), i64::try_from(position + 1).unwrap(),
                    format!("answer {id} revision {version}"), "root".into()));
            }
            let pairs: Vec<_> = expected.iter().map(|row| (row.0.as_str(), row.2.as_str())).collect();
            let mut chat = chat_with_messages(&pairs);
            for (index, message) in chat.messages.inline_mut().iter_mut().enumerate() {
                message.context = if index == 0 { MessageContext::Root }
                    else { MessageContext::Parent { id: "root".into() } };
                if index != 0 { message.role = crate::role::Role::Assistant; }
            }
            storage.replace_chat(&chat).unwrap();
            check(&storage, &expected);
            let revision = storage.silver_certificate().unwrap().revision;
            let retry = storage.replace_chat(&chat).unwrap();
            prop_assert!(retry.unchanged);
            prop_assert_eq!(storage.silver_certificate().unwrap().revision, revision);
            check(&storage, &expected);
            storage.commit_writer().unwrap();
            drop(storage);
            storage = Storage::open_writer(&path).unwrap();
            check(&storage, &expected);
        }
        storage.commit_writer().unwrap();
        drop(storage);
        let archive = directory.path().join("archive");
        crate::export::write(&path, &archive, None).unwrap();
        let imported = directory.path().join("imported.sqlite3");
        crate::export::load(&archive, &imported, true).unwrap();
        let restored = Storage::open_silver(&imported).unwrap();
        check(&restored, &expected);
    }
}
