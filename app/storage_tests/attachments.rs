use crate::storage::Storage;
use crate::storage::attachments::verification_path;
use crate::storage::generated as sql;
use crate::{
    attachments::{self, Availability, Metadata},
    provider::{Provider, ProviderRoots},
    silver::{self, IndexOptions},
};
use std::path::PathBuf;

#[test]
fn changing_attachment_roots_replays_unchanged_sources_and_preserves_visible_text() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("session.jsonl");
    let files = root.path().join("files");
    std::fs::create_dir(&files).unwrap();
    let image = files.join("photo.png");
    std::fs::write(&image, [137, 80, 78, 71]).unwrap();
    std::fs::write(&source, serde_json::to_string(&serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn","item":{"type":"UserMessage","id":"message","content":[{"type":"text","text":"real request"},{"type":"local_image","path":image}]}}})).unwrap()).unwrap();
    let options = IndexOptions {
        providers: ProviderRoots::from([(Provider::Codex, vec![root.path().to_path_buf()])]),
        roots_are_authoritative: true,
        since_days: None,
    };
    let db = root.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&db).unwrap();
    writer
        .configure_attachments(attachments::Configuration::default())
        .unwrap();
    silver::index(&mut writer, &options).unwrap();
    let id = sql::AttachmentCandidates
        .query_many(&writer.connection)
        .unwrap()
        .remove(0)
        .id;
    assert_eq!(
        writer.list_attachments(&id).unwrap().attachments[0].availability,
        Availability::NotAllowed
    );
    writer
        .configure_attachments(attachments::Configuration {
            allowed_roots: vec![files.clone()],
            share_files: false,
        })
        .unwrap();
    silver::index(&mut writer, &options).unwrap();
    assert_eq!(
        writer.list_attachments(&id).unwrap().attachments[0].availability,
        Availability::Retained
    );
    let visible: String = writer
        .connection
        .query_row(
            "SELECT search_projection FROM messages WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(visible, "real request");
    let metadata = writer.list_attachments(&id).unwrap().attachments.remove(0);
    std::fs::remove_file(attachments::directory(&db).join(metadata.digest.as_ref().unwrap()))
        .unwrap();
    assert_eq!(
        writer.list_attachments(&id).unwrap().attachments[0].availability,
        Availability::Missing
    );
    writer
        .configure_attachments(attachments::Configuration {
            allowed_roots: vec![files],
            share_files: false,
        })
        .unwrap();
    silver::index(&mut writer, &options).unwrap();
    assert!(writer.get_attachment(&id, &metadata.id).is_ok());
    writer.commit_writer().unwrap();
}

/// Whether configuring reused the process's earlier cache verification.
fn configure_traced(writer: &mut Storage, config: &attachments::Configuration) -> bool {
    let (bytes, dispatcher) = crate::observability::test_support::capture();
    tracing::dispatcher::with_default(&dispatcher, || writer.configure_attachments(config.clone()))
        .unwrap();
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    output.lines().any(|line| {
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        event["span"]["name"] == "verify_attachment_cache"
            && event["span"]["memoized"] == serde_json::Value::Bool(true)
    })
}

/// One Codex chat whose message retains one allowed image.
struct Fixture {
    _root: tempfile::TempDir,
    db: PathBuf,
    options: IndexOptions,
    config: attachments::Configuration,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("session.jsonl");
        let files = root.path().join("files");
        std::fs::create_dir(&files).unwrap();
        let image = files.join("photo.png");
        std::fs::write(&image, [137, 80, 78, 71]).unwrap();
        std::fs::write(&source, serde_json::to_string(&serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn","item":{"type":"UserMessage","id":"message","content":[{"type":"text","text":"real request"},{"type":"local_image","path":image}]}}})).unwrap()).unwrap();
        Self {
            options: IndexOptions {
                providers: ProviderRoots::from([(
                    Provider::Codex,
                    vec![root.path().to_path_buf()],
                )]),
                roots_are_authoritative: true,
                since_days: None,
            },
            config: attachments::Configuration {
                allowed_roots: vec![files],
                share_files: false,
            },
            db: root.path().join("index.sqlite3"),
            _root: root,
        }
    }

    /// The retained message and its attachment after a first committed index.
    fn indexed(&self) -> (String, Metadata) {
        let mut writer = Storage::open_writer(&self.db).unwrap();
        writer.configure_attachments(self.config.clone()).unwrap();
        silver::index(&mut writer, &self.options).unwrap();
        let id = sql::AttachmentCandidates
            .query_many(&writer.connection)
            .unwrap()
            .remove(0)
            .id;
        let metadata = writer.list_attachments(&id).unwrap().attachments.remove(0);
        assert_eq!(metadata.availability, Availability::Retained);
        writer.commit_writer().unwrap();
        (id, metadata)
    }

    fn remove_cached(&self, metadata: &Metadata) {
        std::fs::remove_file(
            attachments::directory(&self.db).join(metadata.digest.as_ref().unwrap()),
        )
        .unwrap();
    }
}

fn replayed(writer: &Storage) -> i64 {
    writer
        .connection
        .query_row(
            "SELECT count(*) FROM source_checkpoints WHERE interpretation_version = 0",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
}

#[test]
fn repeated_refreshes_reuse_cache_verification_until_a_file_is_removed() {
    let fixture = Fixture::new();
    let (config, options, db) = (&fixture.config, &fixture.options, &fixture.db);
    let mut writer = Storage::open_writer(db).unwrap();
    // The first verification reads silver; the writer's own indexing,
    // including the file it retains, extends it rather than forcing another.
    assert!(!configure_traced(&mut writer, config));
    silver::index(&mut writer, options).unwrap();
    let id = sql::AttachmentCandidates
        .query_many(&writer.connection)
        .unwrap()
        .remove(0)
        .id;
    let metadata = writer.list_attachments(&id).unwrap().attachments.remove(0);
    assert_eq!(metadata.availability, Availability::Retained);
    assert!(configure_traced(&mut writer, config));
    assert!(configure_traced(&mut writer, config));
    assert_eq!(replayed(&writer), 0);
    fixture.remove_cached(&metadata);
    // Removal is detected even if the directory time did not visibly change.
    assert!(!configure_traced(&mut writer, config));
    assert_eq!(replayed(&writer), 1, "a removed cached file forces replay");
    silver::index(&mut writer, options).unwrap();
    assert!(writer.get_attachment(&id, &metadata.id).is_ok());
    writer.commit_writer().unwrap();
}

#[test]
fn a_new_process_reuses_the_committed_verification_and_detects_removal() {
    let fixture = Fixture::new();
    let (id, metadata) = fixture.indexed();
    // A fresh writer has no verification of its own: it reads the record
    // its predecessor's commit wrote instead of the archive.
    let mut writer = Storage::open_writer(&fixture.db).unwrap();
    assert!(writer.attachment_verification.is_none());
    assert!(configure_traced(&mut writer, &fixture.config));
    silver::index(&mut writer, &fixture.options).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let mut writer = Storage::open_writer(&fixture.db).unwrap();
    assert!(configure_traced(&mut writer, &fixture.config));
    assert_eq!(replayed(&writer), 0);
    writer.commit_writer().unwrap();
    drop(writer);

    fixture.remove_cached(&metadata);
    let mut writer = Storage::open_writer(&fixture.db).unwrap();
    assert!(!configure_traced(&mut writer, &fixture.config));
    assert_eq!(replayed(&writer), 1, "a removed cached file forces replay");
    silver::index(&mut writer, &fixture.options).unwrap();
    assert!(writer.get_attachment(&id, &metadata.id).is_ok());
    writer.commit_writer().unwrap();
}

#[test]
fn another_writers_attachment_change_invalidates_the_recorded_verification() {
    let fixture = Fixture::new();
    let (id, metadata) = fixture.indexed();
    // Another connection retains a digest the cache never held.
    let connection = rusqlite::Connection::open(&fixture.db).unwrap();
    crate::storage::packed::register(&connection).unwrap();
    let mut phantom = metadata.clone();
    phantom.id = "f".repeat(64);
    phantom.digest = Some("e".repeat(64));
    connection
        .execute(
            "UPDATE messages SET attachments=?1 WHERE id=?2",
            [serde_json::to_string(&[metadata, phantom]).unwrap(), id],
        )
        .unwrap();
    drop(connection);
    let mut writer = Storage::open_writer(&fixture.db).unwrap();
    assert!(!configure_traced(&mut writer, &fixture.config));
    assert_eq!(
        replayed(&writer),
        1,
        "an unverified retained digest forces replay"
    );
}

#[test]
fn an_uncommitted_verification_is_never_recorded() {
    let fixture = Fixture::new();
    fixture.indexed();
    let record = verification_path(&fixture.db);
    std::fs::remove_file(&record).unwrap();
    let mut writer = Storage::open_writer(&fixture.db).unwrap();
    assert!(!configure_traced(&mut writer, &fixture.config));
    silver::index(&mut writer, &fixture.options).unwrap();
    drop(writer);
    assert!(!record.exists(), "a rolled-back writer records nothing");
}
