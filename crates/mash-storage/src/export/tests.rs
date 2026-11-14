use super::*;
use rusqlite::{Connection, types::Value};

#[test]
fn portable_v2_parent_positions_resolve_after_out_of_order_records() {
    let root = tempfile::tempdir().unwrap();
    let (database, connection) = fixture(root.path());
    connection.execute_batch(
        "INSERT INTO messages(id,chat_id,ordinal,role,content_packed,search_projection,context_parent,created_at,fingerprint)
         VALUES('reply','thread',2,'assistant','Reply','Reply','message',12,'reply-hash');"
    ).unwrap();
    crate::storage::turns::derive(&connection, "thread").unwrap();
    drop(connection);
    let archive = root.path().join("export");
    write(&database, &archive, None).unwrap();
    let path = archive.join(DATABASE);
    let mut records: Vec<crate::silver::archive::Record> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let parent = records
        .iter()
        .position(|record| {
            matches!(record,
                crate::silver::archive::Record::Message { id, .. } if id == "message"
            )
        })
        .unwrap();
    let child = records.iter().position(|record| matches!(record,
        crate::silver::archive::Record::Message { id, context: crate::silver::archive::Context::Parent { ordinal: 0 }, .. } if id == "reply"
    )).unwrap();
    assert!(parent < child);
    records.swap(parent, child);
    for missing_parent in [false, true] {
        if missing_parent {
            let crate::silver::archive::Record::Message { context, .. } = &mut records[parent]
            else {
                panic!("expected child record");
            };
            *context = crate::silver::archive::Context::Parent { ordinal: 1 };
        }
        let text = records
            .iter()
            .map(|record| serde_json::to_string(record).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(&path, text).unwrap();
        let mut manifest: Manifest =
            serde_json::from_slice(&fs::read(archive.join(MANIFEST)).unwrap()).unwrap();
        manifest.records = digest(&path).unwrap();
        fs::write(
            archive.join(MANIFEST),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let imported = root
            .path()
            .join(format!("imported-{missing_parent}.sqlite3"));
        let result = load(&archive, &imported, true);
        if missing_parent {
            assert!(result.is_err());
            assert!(!imported.exists());
        } else {
            result.unwrap();
            let connection = crate::storage::test_support::connect(&imported);
            let parent: String = connection
                .query_row(
                    "SELECT context_parent FROM messages WHERE id='reply'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(parent, "message");
        }
    }
}

#[test]
fn import_preserves_past_interpretation_evidence_without_certifying_current_views() {
    let root = tempfile::tempdir().unwrap();
    let (database, connection) = fixture(root.path());
    connection
        .execute_batch(
            "UPDATE silver_state SET interpretation_version=2;
         UPDATE source_checkpoints SET interpretation_version=2;
         UPDATE silver_scans SET interpretation_version=2;",
        )
        .unwrap();
    drop(connection);
    let archive = root.path().join("export");
    write(&database, &archive, None).unwrap();
    let path = archive.join(MANIFEST);
    let mut manifest: Manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    // Model the certificate emitted by the release that certified interpretation 2.
    manifest.silver.completeness = crate::silver::certificate::Completeness::Complete;
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let imported = root.path().join("imported.sqlite3");
    let result = load(&archive, &imported, true).unwrap();
    assert_eq!(result.counts, manifest.counts);
    assert_eq!(result.silver.revision, manifest.silver.revision);
    assert_eq!(result.silver.interpretation_version, 2);
    assert_eq!(
        result.silver.completeness,
        crate::silver::certificate::Completeness::Unverified
    );
    assert!(!result.search_ready);
    assert!(matches!(result.recommended_action, NextStep::RepairSilver));
    manifest.silver.revision += 1;
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let rejected = root.path().join("rejected.sqlite3");
    assert!(load(&archive, &rejected, true).is_err());
    assert!(!rejected.exists());
}

#[test]
fn portable_records_reject_corrupt_structure_even_with_a_matching_file_digest() {
    for fault in [
        "unknown_field",
        "unknown_record",
        "retired_external_record",
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
            "retired_external_record" => lines.insert(
                1,
                r#"{"record":"connector_source","source_id":"slack","revision":1}"#.into(),
            ),
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
        assert!(load(&exported, &destination, false).is_err(), "{fault}");
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
         INSERT INTO chats(id,provider,title,project,repository,created_at,updated_at,source_path,source_key,source_fingerprint)
         VALUES('thread','codex','Useful thread','/project','https://example.test/repo',10,20,'/missing/session','file:/missing/session','source-hash');
         INSERT INTO messages(id,chat_id,ordinal,role,content_packed,search_projection,context_parent,created_at,metadata,fingerprint)
         VALUES('message','thread',0,'user','Canonical evidence','visible evidence','',11,
           '{\"tool_calls\":[{\"name\":\"exec\",\"call_id\":\"c1\",\"input\":{\"cmd\":[\"ls\"]}}],\"tool_results\":[{\"call_id\":\"c0\",\"is_error\":true}]}','message-hash');
         INSERT INTO chat_enrichment VALUES('thread','{\"version\":1,\"title\":null,\"project\":null,\"repository\":null}');
         INSERT INTO tombstones VALUES('codex','forgotten-thread',30);
         INSERT INTO claude_sources VALUES('branch','/missing/branch','thread','Branch title','/other-project',NULL);
         INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content_packed,projection,created_at,metadata)
         VALUES('branch','branch-message',NULL,'assistant','branch evidence','visible branch',15,'{\"tool_results\":[{\"call_id\":\"toolu_1\"}]}');
         INSERT INTO source_checkpoints(provider,source_path,source_key,size_bytes,modified_ns,fingerprint,interpretation_version) VALUES('codex','/missing/session','file:/missing/session',99,123,'checkpoint-hash',3);
         INSERT INTO silver_scans VALUES('codex',3,1);
         INSERT INTO attachment_policy VALUES(1,'policy-fingerprint');
         UPDATE shared_publication SET acknowledged=revision,target='old-service';
         UPDATE silver_state SET revision=77,interpretation_version=3;"
    ).unwrap();
    // Indexing derives turns; the raw fixture does the same, then keeps its
    // chosen revisions.
    crate::storage::turns::derive(&connection, "thread").unwrap();
    connection
        .execute_batch(
            "UPDATE silver_state SET revision=77;
             UPDATE shared_publication SET acknowledged=revision;",
        )
        .unwrap();
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
    load(&exported, &imported, false).unwrap();
    let restored = crate::storage::test_support::connect(&imported);
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
    let sql = match table {
        "messages" => {
            "SELECT id,chat_id,ordinal,role,context_parent,turn_id,content,
            search_projection,created_at,metadata,attachments,fingerprint FROM messages ORDER BY id"
                .to_owned()
        }
        // Packing is a storage detail; the decoded text is what must survive.
        "claude_source_messages" => "SELECT attachments,metadata,source_key,message_id,parent_id,
            role,content,projection,created_at FROM claude_source_messages ORDER BY 3,4"
            .to_owned(),
        _ => format!("SELECT * FROM {table} ORDER BY 1"),
    };
    let mut statement = connection.prepare(&sql).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns).map(|column| row.get(column)).collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Upgrades export with the old release and import with the new one; the
/// node keeps publishing as itself, starting from a full snapshot.
#[test]
fn upgrade_import_keeps_publication_identity_and_ignores_retired_sqlite_gold() {
    let root = tempfile::tempdir().unwrap();
    let (database, writer) = fixture(root.path());
    let identity = |connection: &Connection| -> (String, i64, i64, String) {
        connection
            .query_row(
                "SELECT instance,revision,acknowledged,target FROM shared_publication",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    };
    let (instance, revision, _, _) = identity(&writer);
    let export = root.path().join("export");
    write(&database, &export, Some("source-node".into())).unwrap();
    let destination = root.path().join("upgraded.sqlite3");
    let views = root.path().join("upgraded.views.sqlite3");
    std::fs::write(&views, b"stale views").unwrap();
    load(&export, &destination, true).unwrap();
    assert_eq!(std::fs::read(&views).unwrap(), b"stale views");
    let imported = crate::storage::test_support::connect(&destination);
    assert_eq!(identity(&imported), (instance, revision, 0, String::new()));
    let floor: i64 = imported
        .query_row("SELECT floor FROM shared_change_state", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(floor, revision);
}

#[test]
fn round_trip_preserves_wal_silver_attachments_and_deletion_evidence_without_views_or_acks() {
    let root = tempfile::tempdir().unwrap();
    let (database, writer) = fixture(root.path());
    // A live WAL writer has an uncommitted edit. Only committed silver is copied.
    writer
        .execute_batch("BEGIN IMMEDIATE; UPDATE chats SET title='not committed';")
        .unwrap();
    let before = Storage::open_silver(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    let export = root.path().join("export");
    let created = write(&database, &export, Some("source-node".into())).unwrap();
    assert_eq!(created.silver, before);
    assert_eq!(created.counts.attachment_files, 1);
    writer.execute_batch("ROLLBACK").unwrap();
    let destination = root.path().join("imported.sqlite3");
    let result = load(&export, &destination, false).unwrap();
    assert!(!result.search_ready);
    assert!(matches!(
        result.recommended_action,
        NextStep::RebuildLocalViews
    ));
    let imported = crate::storage::test_support::connect(&destination);
    for table in [
        "chats",
        "messages",
        "turns",
        "chat_enrichment",
        "tombstones",
        "claude_sources",
        "claude_source_messages",
        "source_checkpoints",
        "silver_state",
        "silver_scans",
        "silver_rejections",
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
    let history = Storage::open_silver(&destination).unwrap();
    storage::export::validate(&history).unwrap();
    assert!(!root.path().join("imported.views.sqlite3").exists());
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
    let result = load(&export, &root.path().join("imported.sqlite3"), false).unwrap();
    assert!(matches!(result.recommended_action, NextStep::RepairSilver));
    assert_eq!(
        result.silver,
        Storage::open_silver(&database)
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
        assert!(load(&export, &target, false).is_err(), "{failure}");
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
    let before = Storage::open_silver(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    assert!(load(&export, &database, false).is_err());
    assert_eq!(
        before,
        Storage::open_silver(&database)
            .unwrap()
            .silver_certificate()
            .unwrap()
    );
    for suffix in ["", "-wal", "-shm", "-journal", ".attachments"] {
        let target = root.path().join("new.sqlite3");
        let file = root.path().join(format!("new.sqlite3{suffix}"));
        fs::write(&file, b"keep me").unwrap();
        assert!(load(&export, &target, false).is_err());
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
        .execute_batch("DELETE FROM chats; DELETE FROM silver_scans; UPDATE silver_state SET interpretation_version=0;")
        .unwrap();
    let export = root.path().join("export");
    write(&database, &export, None).unwrap();
    let target = root.path().join("imported.sqlite3");
    let result = load(&export, &target, false).unwrap();
    assert!(matches!(result.recommended_action, NextStep::RepairSilver));
    assert_eq!(result.counts.messages, 0);
    let imported = crate::storage::test_support::connect(&target);
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
    assert!(load(&export, &target, false).is_err());
    assert!(!target.exists());
    assert!(!attachments::directory(&target).exists());
}
