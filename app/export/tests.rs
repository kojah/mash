use super::*;
use rusqlite::{Connection, types::Value};

#[test]
fn portable_records_reject_corrupt_structure_even_with_a_matching_file_digest() {
    for fault in [
        "unknown_field",
        "unknown_record",
        "missing_state",
        "after_state",
        "duplicate",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (database, _writer) = fixture(root.path());
        let exported = root.path().join("export");
        write(&database, &exported, None).unwrap();
        let path = exported.join(DATABASE);
        let original = fs::read_to_string(&path).unwrap();
        assert!(!original.starts_with("SQLite format"));
        assert!(!original.contains("storage_id"));
        let mut lines = original.lines().map(str::to_owned).collect::<Vec<_>>();
        match fault {
            "unknown_field" => lines[0].insert_str(1, "\"embedding\":[1,0],"),
            "unknown_record" => lines[0] = "{\"record\":\"invented\"}".into(),
            "missing_state" => {
                lines.pop();
            }
            "after_state" => lines.push(lines[0].clone()),
            "duplicate" => lines.insert(0, lines[0].clone()),
            _ => unreachable!(),
        }
        fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
        let mut manifest: Manifest =
            serde_json::from_slice(&fs::read(exported.join(MANIFEST)).unwrap()).unwrap();
        manifest.records = digest(&path).unwrap();
        fs::write(
            exported.join(MANIFEST),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let destination = root.path().join("new.sqlite3");
        assert!(load(&exported, &destination).is_err(), "{fault}");
        assert!(!destination.exists(), "{fault}");
        assert!(!attachments::directory(&destination).exists(), "{fault}");
    }
}

fn fixture(root: &Path) -> (PathBuf, Connection) {
    let path = root.join("history # ü.sqlite3");
    drop(Storage::open(&path).unwrap());
    let connection = crate::storage::test_support::connect(&path);
    connection.execute_batch(
        "PRAGMA foreign_keys=ON;
         INSERT INTO conversations(id,provider,source_path,source_key,source_session,title,project,repository,source_fingerprint,created_at,updated_at)
         VALUES('thread','codex','/missing/session','file:/missing/session',NULL,'Useful thread','/project','https://example.test/repo','source-hash',10,20);
         INSERT INTO messages(id,conversation_id,ordinal,role,content,search_projection,context_parent,created_at,fingerprint)
         VALUES('message','thread',0,'user','Canonical evidence','visible evidence',-1,11,'message-hash');
         INSERT INTO conversation_enrichment VALUES('thread','{\"version\":1,\"title\":null,\"project\":null,\"repository\":null}');
         INSERT INTO tombstones VALUES('codex','forgotten-thread',30);
         INSERT INTO claude_sources VALUES('branch','/missing/branch','thread','Branch title','/other-project',NULL);
         INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content,projection,created_at)
         VALUES('branch','branch-message',NULL,'assistant','branch evidence','visible branch',15);
         INSERT INTO source_checkpoints(provider,source_path,source_key,size_bytes,modified_ns,fingerprint,interpretation_version) VALUES('codex','/missing/session','file:/missing/session',99,123,'checkpoint-hash',2);
         INSERT INTO silver_scans VALUES('codex',2,1);
         INSERT INTO attachment_policy VALUES(1,'policy-fingerprint');
         INSERT INTO shared_forgets VALUES('shared','forgotten-thread');
         INSERT INTO message_embeddings VALUES('message','old-model',1,1,X'01');
         UPDATE gold_manifest SET manifest='{}';
         UPDATE lexical_manifest SET manifest='{}';
         INSERT INTO message_fts VALUES('visible evidence','message','thread');
         UPDATE derived_state SET search_dirty=0,semantic_ready_generation='old-model';
         UPDATE shared_publication SET acknowledged=revision,target='old-service';
         UPDATE silver_state SET revision=77,interpretation_version=2;
         UPDATE sqlite_sequence SET seq=42 WHERE name='messages';"
    ).unwrap();
    let bytes = b"retained attachment";
    let hash = blake3::hash(bytes).to_hex().to_string();
    let metadata = attachments::Metadata {
        id: "a".repeat(64),
        name: "note.txt".into(),
        media_type: "text/plain".into(),
        availability: attachments::Availability::Retained,
        bytes: Some(bytes.len() as u64),
        digest: Some(hash.clone()),
        redacted: true,
    };
    let json = serde_json::to_string(std::slice::from_ref(&metadata)).unwrap();
    connection
        .execute("UPDATE messages SET attachments=?1", [&json])
        .unwrap();
    let source_json = serde_json::to_string(&[attachments::Source::Retained { metadata }]).unwrap();
    connection
        .execute(
            "UPDATE claude_source_messages SET attachments=?1",
            [&source_json],
        )
        .unwrap();
    fs::create_dir(attachments::directory(&path)).unwrap();
    fs::write(attachments::directory(&path).join(hash), bytes).unwrap();
    (path, connection)
}

#[test]
fn export_keeps_retained_files_from_nonvisible_claude_branches() {
    let root = tempfile::tempdir().unwrap();
    let (database, writer) = fixture(root.path());
    writer
        .execute("UPDATE messages SET attachments='[]'", [])
        .unwrap();
    let exported = root.path().join("export");
    let result = write(&database, &exported, None).unwrap();
    assert_eq!(result.counts.attachment_files, 1);
    let imported = root.path().join("imported.sqlite3");
    load(&exported, &imported).unwrap();
    let restored = Connection::open(&imported).unwrap();
    assert_eq!(
        rows(&restored, "claude_source_messages"),
        rows(&writer, "claude_source_messages")
    );
    let files = fs::read_dir(attachments::directory(&imported))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(fs::read(files[0].path()).unwrap(), b"retained attachment");
}

#[test]
fn export_rejects_unresolved_provider_attachments_in_stored_claude_evidence() {
    let root = tempfile::tempdir().unwrap();
    let (database, writer) = fixture(root.path());
    let unresolved = serde_json::to_string(&[attachments::Source::File {
        name: "private.txt".into(),
        media_type: "text/plain".into(),
        path: root.path().join("must-not-be-read"),
    }])
    .unwrap();
    writer
        .execute(
            "UPDATE claude_source_messages SET attachments=?1",
            [&unresolved],
        )
        .unwrap();
    let exported = root.path().join("export");
    let error = write(&database, &exported, None).unwrap_err();
    assert_eq!(error.error.message, "invalid export attachment metadata");
    assert!(!exported.exists());
}

fn rows(connection: &Connection, table: &str) -> Vec<Vec<Value>> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
        .unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns).map(|column| row.get(column)).collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn round_trip_preserves_wal_silver_attachments_and_deletion_evidence_without_gold_or_acks() {
    let root = tempfile::tempdir().unwrap();
    let (database, writer) = fixture(root.path());
    // A live WAL writer has an uncommitted edit. Only committed silver is copied.
    writer
        .execute_batch("BEGIN IMMEDIATE; UPDATE conversations SET title='not committed';")
        .unwrap();
    let before = Storage::open_catalog(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    let export = root.path().join("export");
    let created = write(&database, &export, Some("source-node".into())).unwrap();
    assert_eq!(created.silver, before);
    assert_eq!(created.counts.attachment_files, 1);
    writer.execute_batch("ROLLBACK").unwrap();
    let destination = root.path().join("imported.sqlite3");
    let result = load(&export, &destination).unwrap();
    assert!(!result.search_ready);
    assert!(matches!(
        result.recommended_action,
        NextStep::RebuildLocalGold
    ));
    let imported = Connection::open(&destination).unwrap();
    for table in [
        "conversations",
        "messages",
        "conversation_enrichment",
        "tombstones",
        "claude_sources",
        "claude_source_messages",
        "source_checkpoints",
        "silver_state",
        "silver_scans",
        "silver_rejections",
        "shared_forgets",
        "attachment_policy",
    ] {
        assert_eq!(rows(&writer, table), rows(&imported, table), "{table}");
    }
    let old_instance: String = writer
        .query_row("SELECT instance FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    let new_instance: String = imported
        .query_row("SELECT instance FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_ne!(old_instance, new_instance);
    let history = Storage::open_catalog(&destination).unwrap();
    storage::export::validate(&history).unwrap();
    assert!(Storage::open_lexical(&destination).is_err());
    assert!(Storage::open_search(&destination, "old-model").is_err());
    let attachment = history
        .list_attachments("message")
        .unwrap()
        .attachments
        .remove(0);
    assert_eq!(
        history
            .get_attachment("message", &attachment.id)
            .unwrap()
            .data,
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"retained attachment"
        )
    );
    assert_eq!(before, history.silver_certificate().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&export).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for file in [export.join(DATABASE), export.join(MANIFEST), destination] {
            assert_eq!(
                fs::metadata(file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn pending_ingestion_is_preserved_instead_of_becoming_search_ready() {
    let root = tempfile::tempdir().unwrap();
    let (database, connection) = fixture(root.path());
    connection.execute_batch("UPDATE silver_scans SET state=0; INSERT INTO silver_rejections VALUES('codex','bad','/missing/bad',123)").unwrap();
    let export = root.path().join("export");
    write(&database, &export, None).unwrap();
    let result = load(&export, &root.path().join("imported.sqlite3")).unwrap();
    assert!(matches!(result.recommended_action, NextStep::RepairSilver));
    assert_eq!(
        result.silver,
        Storage::open_catalog(&database)
            .unwrap()
            .silver_certificate()
            .unwrap()
    );
}

#[test]
fn corrupted_missing_and_incompatible_exports_never_install_a_database() {
    for failure in [
        "database",
        "attachment",
        "missing",
        "manifest",
        "version",
        "counts",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (database, _) = fixture(root.path());
        let export = root.path().join("export");
        write(&database, &export, None).unwrap();
        let mut manifest: Manifest =
            serde_json::from_slice(&fs::read(export.join(MANIFEST)).unwrap()).unwrap();
        match failure {
            "database" => fs::write(export.join(DATABASE), b"corrupt").unwrap(),
            "attachment" | "missing" => {
                let file = fs::read_dir(export.join("attachments"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                if failure == "missing" {
                    fs::remove_file(file).unwrap();
                } else {
                    fs::write(file, b"corrupt").unwrap();
                }
            }
            "manifest" => fs::remove_file(export.join(MANIFEST)).unwrap(),
            "version" => {
                manifest.format_version += 1;
                fs::write(
                    export.join(MANIFEST),
                    serde_json::to_vec(&manifest).unwrap(),
                )
                .unwrap();
            }
            "counts" => {
                manifest.counts.messages += 1;
                fs::write(
                    export.join(MANIFEST),
                    serde_json::to_vec(&manifest).unwrap(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let target = root.path().join("imported.sqlite3");
        assert!(load(&export, &target).is_err(), "{failure}");
        assert!(!target.exists());
        assert!(!attachments::directory(&target).exists());
    }
}

#[test]
fn existing_destinations_and_sidecars_are_never_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let (database, _) = fixture(root.path());
    let export = root.path().join("export");
    write(&database, &export, None).unwrap();
    let original = fs::read(export.join(MANIFEST)).unwrap();
    assert!(write(&database, &export, None).is_err());
    assert_eq!(original, fs::read(export.join(MANIFEST)).unwrap());
    let before = Storage::open_catalog(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    assert!(load(&export, &database).is_err());
    assert_eq!(
        before,
        Storage::open_catalog(&database)
            .unwrap()
            .silver_certificate()
            .unwrap()
    );
    for suffix in ["", "-wal", "-shm", "-journal", ".attachments"] {
        let target = root.path().join("new.sqlite3");
        let file = root.path().join(format!("new.sqlite3{suffix}"));
        fs::write(&file, b"keep me").unwrap();
        assert!(load(&export, &target).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"keep me");
        fs::remove_file(file).unwrap();
    }
}

#[test]
fn missing_retained_files_fail_export_without_leaving_a_completed_archive() {
    let root = tempfile::tempdir().unwrap();
    let (database, _) = fixture(root.path());
    fs::remove_dir_all(attachments::directory(&database)).unwrap();
    let export = root.path().join("export");
    assert!(write(&database, &export, None).is_err());
    assert!(!export.exists());
}

#[test]
fn empty_history_preserves_unverified_state_without_storage_engine_sequences() {
    let root = tempfile::tempdir().unwrap();
    let (database, connection) = fixture(root.path());
    connection
        .execute_batch("DELETE FROM messages; DELETE FROM conversations; DELETE FROM silver_scans; UPDATE silver_state SET interpretation_version=0;")
        .unwrap();
    let export = root.path().join("export");
    write(&database, &export, None).unwrap();
    let target = root.path().join("imported.sqlite3");
    let result = load(&export, &target).unwrap();
    assert!(matches!(result.recommended_action, NextStep::RepairSilver));
    assert_eq!(result.counts.messages, 0);
    let imported = Connection::open(target).unwrap();
    assert_eq!(rows(&imported, "messages").len(), 0);
    let text = fs::read_to_string(export.join(DATABASE)).unwrap();
    assert!(!text.contains("sqlite_sequence"));
    assert!(!text.contains("storage_id"));
}

#[cfg(unix)]
#[test]
fn archive_file_symlinks_are_rejected_without_creating_a_database() {
    let root = tempfile::tempdir().unwrap();
    let (database, _) = fixture(root.path());
    let export = root.path().join("export");
    write(&database, &export, None).unwrap();
    let stored = export.join(DATABASE);
    let moved = root.path().join("outside.sqlite3");
    fs::rename(&stored, &moved).unwrap();
    std::os::unix::fs::symlink(moved, stored).unwrap();
    let target = root.path().join("imported.sqlite3");
    assert!(load(&export, &target).is_err());
    assert!(!target.exists());
    assert!(!attachments::directory(&target).exists());
}
