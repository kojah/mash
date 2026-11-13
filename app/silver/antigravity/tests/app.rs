//! The Antigravity 2.31 app format: `user_version` 1, the agent manager's
//! trajectory source and workspace URIs, in WAL databases closed without a
//! `-wal` file. Synthetic rows mirror the structure of real app snapshots.
use super::*;
use crate::silver::diagnostics::Category;

const WORKSPACE: &str = "file:///work/synthetic-app";

/// Rewrites a native IDE snapshot as the app writes it.
fn to_app(path: &Path) {
    let connection = Connection::open(path).unwrap();
    let bytes: Vec<u8> = connection
        .query_row("SELECT data FROM trajectory_metadata_blob", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut metadata = proto::Metadata::decode(bytes.as_slice()).unwrap();
    metadata.workspaces.clear();
    metadata.workspace_uris = vec![WORKSPACE.into()];
    connection
        .execute(
            "UPDATE trajectory_metadata_blob SET data=?1",
            [metadata.encode_to_vec()],
        )
        .unwrap();
    connection
        .execute_batch(
            "UPDATE trajectory_meta SET source=21; PRAGMA user_version=1; PRAGMA journal_mode=WAL;",
        )
        .unwrap();
    drop(connection);
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    assert!(
        !Path::new(&wal).exists(),
        "a closed WAL database has no -wal"
    );
}

#[test]
fn app_snapshots_read_like_ide_snapshots_with_their_workspace() {
    let ide = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    let expected = chat(&write(ide.path()));
    let path = write(app.path());
    to_app(&path);
    #[cfg(unix)]
    {
        // A read-only connection cannot create the WAL index here.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(app.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let parsed = chat(&path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(app.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let shape = |chat: &Chat| {
        chat.messages
            .inline()
            .iter()
            .map(|m| {
                (
                    m.id.clone(),
                    m.ordinal,
                    m.role.clone(),
                    m.content.clone(),
                    m.dialogue_text.clone(),
                    m.created_at,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(parsed.messages.len(), 9);
    assert_eq!(shape(&parsed), shape(&expected));
    assert!(
        parsed
            .messages
            .inline()
            .iter()
            .all(|m| !m.dialogue_text.as_deref().unwrap().is_empty()),
        "agent manager conversations are searchable dialogue"
    );
    assert_eq!(parsed.id, expected.id);
    assert_eq!(parsed.title, expected.title);
    assert!(parsed.title.is_some());
    assert_eq!(
        (parsed.created_at, parsed.updated_at),
        (expected.created_at, expected.updated_at)
    );
    assert!(parsed.created_at.is_some());
    assert_eq!(parsed.project, crate::platform::file_uri_path(WORKSPACE));
}

#[test]
fn mixed_roots_index_readable_snapshots_and_reject_only_unsupported_ones() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("conversations");
    std::fs::create_dir(&root).unwrap();
    write(&root);
    to_app(&write_case(&root, &fixture().fork));
    // Encrypted conversation protobufs sit beside the databases.
    std::fs::write(
        root.join("23f02797-0000-4000-8000-000000000000.pb"),
        [0x98, 0x45, 0xd5],
    )
    .unwrap();
    let staging = directory.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let newer = write_case(&staging, &fixture().fork);
    Connection::open(&newer)
        .unwrap()
        .pragma_update(None, "user_version", 2)
        .unwrap();
    let newer_path = root.join("60000000-0000-4000-8000-000000000002.db");
    std::fs::rename(&newer, &newer_path).unwrap();
    let unreadable = root.join("60000000-0000-4000-8000-000000000003.db");
    std::fs::write(&unreadable, b"not a database").unwrap();

    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    let summary = index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(summary.rejected_sources, 2);
    assert_eq!(summary.malformed_records, 0);
    let mut rejected = summary
        .record_diagnostics
        .samples
        .iter()
        .map(|issue| {
            assert_eq!(issue.category, Category::UnsupportedFormat);
            assert_eq!(issue.provider, Provider::Antigravity);
            issue.source.clone()
        })
        .collect::<Vec<_>>();
    rejected.sort();
    assert_eq!(rejected, vec![newer_path, unreadable]);
    let chats: i64 = crate::storage::test_support::connect(&database)
        .query_row(
            "SELECT count(*) FROM chats WHERE provider='antigravity'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(chats, 2);
    let (state, rejections) = storage.coverage_scan(Provider::Antigravity).unwrap();
    assert_eq!(state, crate::silver::certificate::Completeness::Incomplete);
    assert_eq!(rejections, 2);
}
