use super::*;

enum Damage {
    Missing,
    Text(String),
    Blob(Vec<u8>),
}

#[test]
fn incomplete_or_invalid_records_preserve_previous_index_and_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let db = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&db).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let source = super::super::super::discover(&path).unwrap().remove(0);
    let stamp = source.stamp().unwrap();
    let key = bubble_key("request-1");
    let original: String = connection
        .query_row("SELECT value FROM cursorDiskKV WHERE key=?1", [&key], |r| {
            r.get(0)
        })
        .unwrap();
    for damage in [
        Damage::Missing,
        Damage::Text("invalid JSON".into()),
        Damage::Text("x".repeat(8 * 1024 * 1024 + 1)),
        Damage::Blob(original.as_bytes().to_vec()),
    ] {
        connection
            .execute("DELETE FROM cursorDiskKV WHERE key=?1", [&key])
            .unwrap();
        match damage {
            Damage::Missing => {}
            Damage::Text(value) => {
                connection
                    .execute(
                        "INSERT INTO cursorDiskKV VALUES(?1,?2)",
                        params![&key, value],
                    )
                    .unwrap();
            }
            Damage::Blob(value) => {
                connection
                    .execute(
                        "INSERT INTO cursorDiskKV VALUES(?1,?2)",
                        params![&key, value],
                    )
                    .unwrap();
            }
        }
        let mut storage = Storage::open_writer(&db).unwrap();
        assert!(index(&mut storage, &options(&path)).is_err());
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 6);
        assert!(
            storage
                .source_checkpoint_matches(Provider::Cursor, &source.identity, &stamp)
                .unwrap()
        );
        assert_eq!(
            storage
                .search("cbioportal", 10, Some(Provider::Cursor), None)
                .unwrap()
                .len(),
            2
        );
    }
    connection
        .execute(
            "INSERT OR REPLACE INTO cursorDiskKV VALUES(?1,?2)",
            params![&key, &original],
        )
        .unwrap();
    let mut root: Root = read(&connection, root_key());
    root.full_conversation_headers_only.push(models::Header {
        bubble_id: "request-1".into(),
        kind: Kind::Human,
        created_at: None,
        is_display_only: false,
    });
    write(&connection, root_key(), &root);
    assert!(parsed_error(&path).contains("duplicate"));
    root.full_conversation_headers_only.pop();
    root.version = 19;
    write(&connection, root_key(), &root);
    assert!(parsed_error(&path).contains("root version"));
    root.version = 18;
    root.composer_id = "wrong".into();
    write(&connection, root_key(), &root);
    assert!(parsed_error(&path).contains("composer identity"));
    root.composer_id = "fixture-ide-session".into();
    root.conversation_state = Some("~not-base64".into());
    write(&connection, root_key(), &root);
    assert!(parsed_error(&path).contains("state encoding"));
}
fn parsed_error(path: &Path) -> String {
    let source = super::super::super::discover(path)
        .unwrap()
        .remove(0)
        .identity;
    super::super::super::parse(&source)
        .err()
        .unwrap()
        .error
        .message
}

#[test]
fn structural_and_cumulative_read_limits_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let mut root: Root = read(&connection, root_key());
    let encoded = STANDARD
        .decode(
            root.conversation_state
                .as_deref()
                .unwrap()
                .strip_prefix('~')
                .unwrap(),
        )
        .unwrap();
    root.conversation_state = Some(format!("~{}", STANDARD.encode([0x0a, 0].repeat(65_537))));
    write(&connection, root_key(), &root);
    assert!(parsed_error(&path).contains("field limit"));
    root.conversation_state = Some(format!("~{}", STANDARD.encode(encoded)));
    root.full_conversation_headers_only = (0..16_384)
        .map(|i| models::Header {
            bubble_id: format!("record-{i}"),
            kind: Kind::Human,
            created_at: None,
            is_display_only: false,
        })
        .collect();
    write(&connection, root_key(), &root);
    assert!(parsed_error(&path).contains("structural ingestion limit"));
    root.full_conversation_headers_only.clear();
    root.name = Some("x".repeat(7 * 1024 * 1024));
    write(&connection, root_key(), &root);
    let mut store = Store::open(&path).unwrap();
    for _ in 0..9 {
        let _: Root = store.json(root_key()).unwrap();
    }
    assert!(
        store
            .json::<Root>(root_key())
            .err()
            .unwrap()
            .error
            .message
            .contains("64 MiB")
    );
}

#[test]
fn unambiguous_local_metadata_is_required_and_inline_hex_state_is_supported() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let mut root: Root = read(&connection, root_key());
    let mut state = super::super::state(&root).unwrap();
    state.workspaces.push("file:///other/project".into());
    root.conversation_state = Some(hex::encode(state.encode_to_vec()));
    root.tracked_git_repos.push(models::Repository {
        path: "/other/repository".into(),
    });
    write(&connection, root_key(), &root);
    let conversation = parsed(&path);
    assert_eq!(conversation.project, None);
    assert_eq!(conversation.repository, None);
    let mut bubble: Bubble = read(&connection, &bubble_key("request-1"));
    bubble.created_at = Some("invalid".into());
    write(&connection, &bubble_key("request-1"), &bubble);
    assert!(parsed_error(&path).contains("ISO timestamp"));
}

#[test]
fn windows_repository_paths_are_kept_when_read_on_any_platform() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let mut root: Root = read(&connection, root_key());
    let mut state = super::super::state(&root).unwrap();
    state.repositories.clear();
    root.conversation_state = Some(hex::encode(state.encode_to_vec()));
    root.tracked_git_repos = vec![models::Repository {
        path: "C:\\Users\\Ada\\repository".into(),
    }];
    write(&connection, root_key(), &root);
    assert_eq!(
        parsed(&path).repository.as_deref(),
        Some("C:\\Users\\Ada\\repository")
    );
}
