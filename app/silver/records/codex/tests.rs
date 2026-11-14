use std::io::Write;

fn event(kind: &str, body: &str) -> String {
    format!(
        r#"{{"timestamp":"2026-09-30T00:00:00Z","type":"event_msg","payload":{{"type":"item_completed","item":{{"stdout":"{body}","type":"{kind}","id":"tool-1"}}}}}}"#
    )
}

#[test]
fn real_parser_and_index_preserve_history_across_large_lifecycle_envelopes() {
    use crate::silver::{IndexOptions, index};
    use crate::{provider::Provider, storage::Storage};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("codex.jsonl");
    let message = r#"{"timestamp":"2026-09-30T00:00:00Z","type":"response_item","payload":{"type":"message","id":"user-1","role":"user","content":[{"type":"input_text","text":"cbioportal canonical history"}]}}"#;
    std::fs::write(&path, format!("{message}\n")).unwrap();
    let options = IndexOptions {
        providers: [(Provider::Codex, vec![path.clone()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let database = root.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    // Windows grants an append handle no right to truncate (set_len), even
    // with write(true), so open for writing and seek to the end instead.
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    std::io::Seek::seek(&mut file, std::io::SeekFrom::End(0)).unwrap();
    let large = event(
        "FileChange",
        &"x".repeat(crate::silver::MAX_RECORD_BYTES + 1),
    );
    writeln!(file, "{large}").unwrap();
    let valid_length = file.metadata().unwrap().len();
    let parsed = crate::silver::parsers::parse_codex(&path)
        .unwrap()
        .chat
        .unwrap();
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(
        parsed.messages.inline()[0].content,
        "cbioportal canonical history"
    );
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    let source = crate::silver::SourceIdentity::file(&path);
    let checkpoint = crate::silver::source_stamp_for(&source).unwrap();
    drop(storage);
    // Malformed large mirrors must not replace the previous indexed snapshot.
    file.write_all(&large.as_bytes()[..large.len() - 1])
        .unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(index(&mut storage, &options).is_err());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Codex, &source, &checkpoint)
            .unwrap()
    );
    assert_eq!(
        crate::storage::test_support::projections_with_text(&storage, "cbioportal", None).len(),
        1
    );
    file.set_len(valid_length).unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert!(
        storage
            .source_checkpoint_matches(
                Provider::Codex,
                &source,
                &crate::silver::source_stamp_for(&source).unwrap()
            )
            .unwrap()
    );
}
