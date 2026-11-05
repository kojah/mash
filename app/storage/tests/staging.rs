use super::*;
use crate::silver::messages::Messages;

#[test]
fn staged_refresh_reconciles_removed_and_moved_messages() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let original = conversation_with_messages(&[("a", "alpha"), ("b", "beta"), ("c", "gamma")]);
    storage.replace_conversation(&original).unwrap();
    storage.commit_and_continue_writer().unwrap();
    let mut refreshed = conversation_with_messages(&[
        ("c", "gamma"),
        ("a", "changed canonical context"),
        ("d", "delta"),
    ]);
    refreshed.messages.inline_mut()[1].dialogue_text = Some("alpha".to_owned());
    refreshed.messages = Messages::stage_fixture(refreshed.messages.inline().to_vec()).unwrap();
    let changed = storage.replace_conversation(&refreshed).unwrap();
    assert_eq!(changed.changed_message_ids.len(), 3);
    assert_eq!(changed.removed_messages, 1);
    assert!(storage.view("b", 0).unwrap().is_empty());
    assert_eq!(storage.view("c", 0).unwrap()[0].ordinal, 0);
    assert_eq!(storage.view("a", 0).unwrap()[0].ordinal, 1);
    assert_eq!(
        storage.view("a", 0).unwrap()[0].content,
        "changed canonical context"
    );
    assert!(storage.replace_conversation(&refreshed).unwrap().unchanged);
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 3);
}

#[test]
fn corrupt_staging_cannot_replace_a_valid_canonical_source() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let original = conversation_with_messages(&[("a", "alpha"), ("b", "beta")]);
    storage.replace_conversation(&original).unwrap();
    storage.commit_and_continue_writer().unwrap();
    let mut replacement = conversation_with_messages(&[("a", "changed"), ("c", "new")]);
    replacement.messages = Messages::stage_fixture(replacement.messages.inline().to_vec()).unwrap();
    let first = replacement
        .messages
        .rows()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .into_owned();
    std::fs::write(
        replacement.messages.staging_path().unwrap(),
        serde_json::to_vec(&first).unwrap(),
    )
    .unwrap();
    let error = storage.replace_conversation(&replacement).err().unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Io);
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert_eq!(storage.view("a", 0).unwrap()[0].content, "alpha");
    assert_eq!(storage.view("b", 0).unwrap()[0].content, "beta");
    assert!(storage.view("c", 0).unwrap().is_empty());
}
