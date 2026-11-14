use crate::storage::Storage;
use crate::storage::test_support::chat_with_messages;

#[test]
fn dashboard_local_and_shared_pages_read_the_same_silver_positions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    storage
        .replace_chat(&chat_with_messages(&[
            ("a", "alpha"),
            ("b", "beta"),
            ("c", "gamma"),
        ]))
        .unwrap();
    storage.commit_writer().unwrap();
    for (offset, limit, expected) in [(0, 2, vec!["a", "b"]), (2, 2, vec!["c"]), (3, 2, vec![])] {
        let local = storage.chat_messages("session-1", offset, limit).unwrap();
        let shared = crate::shared::silver_store::read::chat_page(
            &storage.connection,
            &crate::shared::protocol::ChatPage {
                node_id: "node".into(),
                chat_id: "session-1".into(),
                offset: u32::try_from(offset).unwrap(),
                limit: u32::try_from(limit).unwrap(),
            },
        )
        .unwrap();
        assert_eq!(
            local.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            serde_json::to_value(local).unwrap(),
            serde_json::to_value(shared).unwrap()
        );
    }
}
