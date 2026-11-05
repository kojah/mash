use super::*;
use crate::silver::enrichment::{Evidence, Origin, SourceField};

fn expected() -> Evidence {
    Evidence {
        version: crate::silver::enrichment::VERSION,
        timestamps: Some(crate::silver::enrichment::Timestamps {
            created_at: Some(crate::silver::enrichment::TimestampOrigin::SourceMetadata {
                field: crate::silver::enrichment::TimestampField::SessionTimeCreated,
                rule_version: crate::silver::enrichment::VERSION,
            }),
            updated_at: Some(crate::silver::enrichment::TimestampOrigin::SourceMetadata {
                field: crate::silver::enrichment::TimestampField::SessionTimeUpdated,
                rule_version: crate::silver::enrichment::VERSION,
            }),
        }),
        title: Some(Origin::SourceMetadata {
            field: SourceField::SessionTitle,
        }),
        project: Some(Origin::SourceMetadata {
            field: SourceField::SessionDirectory,
        }),
        repository: Some(Origin::SourceMetadata {
            field: SourceField::ProjectWorktree,
        }),
    }
}

#[test]
fn native_metadata_origins_preserve_absence_and_detect_mislabelled_fields() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let upstream = fixture(&path);
    upstream
        .execute_batch("UPDATE project SET vcs=NULL; UPDATE session SET directory='';")
        .unwrap();
    let source = discover(&path).unwrap().remove(0).identity;
    let parsed = parse(&source).unwrap().chat.unwrap();
    let evidence = Evidence::capture(&parsed).unwrap();
    assert_eq!(evidence.title, expected().title);
    assert!(parsed.project.is_none() && evidence.project.is_none());
    assert!(parsed.repository.is_none() && evidence.repository.is_none());
    assert!(evidence.is_current_for(Provider::Opencode));
    let mut incorrect = expected();
    incorrect.project = Some(Origin::SourceMetadata {
        field: SourceField::Cwd,
    });
    assert!(incorrect.is_current());
    assert!(!incorrect.is_current_for(Provider::Opencode));
}

#[test]
fn native_metadata_origins_survive_restart_and_old_unknown_evidence_requires_replay() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let upstream = fixture(&path);
    // A native title equal to a user message must remain a native fact.
    upstream
        .execute(
            "UPDATE session SET title='Fix the cBioPortal importer' WHERE id='alpha'",
            [],
        )
        .unwrap();
    let source = discover(&path).unwrap().remove(0).identity;
    let parsed = parse(&source).unwrap().chat.unwrap();
    assert_eq!(Evidence::capture(&parsed).unwrap(), expected());
    let local = directory.path().join("silver.sqlite3");
    let mut storage = Storage::open_writer(&local).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    let history =
        serde_json::to_vec(&storage.view(&parsed.messages.inline()[0].id, 10).unwrap()).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let stored = Storage::open_silver(&local).unwrap();
    assert_eq!(
        stored.chat_enrichment(&parsed.id).unwrap(),
        Some(expected())
    );
    assert!(
        stored
            .chat_candidates(&crate::chat_catalog::Request::default(), "local")
            .unwrap()
            .iter()
            .all(|thread| thread.metadata.enrichment == Some(expected()))
    );
    let mut old = expected();
    old.timestamps = None;
    old.title = Some(Origin::Unknown);
    old.project = Some(Origin::Unknown);
    old.repository = Some(Origin::Unknown);
    let connection = Connection::open(&local).unwrap();
    connection
        .execute(
            "UPDATE chat_enrichment SET evidence=?1",
            [serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
    drop(connection);
    assert_eq!(
        stored.chat_enrichment(&parsed.id).unwrap(),
        Some(old.clone())
    );
    assert_publication(&stored, &old);
    let before = stored.silver_certificate().unwrap();
    let native_stamp = source_stamp_for(&source).unwrap();
    let mut storage = Storage::open_writer(&local).unwrap();
    assert!(
        !storage
            .source_checkpoint_matches(Provider::Opencode, &source, &native_stamp)
            .unwrap()
    );
    let replayed = index(&mut storage, &options(&path)).unwrap();
    assert_eq!(replayed.checkpoint_skipped_sources, 0);
    assert_eq!(replayed.changed_messages, 0);
    assert_eq!(
        storage.chat_enrichment(&parsed.id).unwrap(),
        Some(expected())
    );
    assert!(storage.silver_certificate().unwrap().revision > before.revision);
    assert_eq!(source_stamp_for(&source).unwrap(), native_stamp);
    assert_eq!(
        serde_json::to_vec(&storage.view(&parsed.messages.inline()[0].id, 10).unwrap()).unwrap(),
        history
    );
    assert_eq!(
        index(&mut storage, &options(&path))
            .unwrap()
            .checkpoint_skipped_sources,
        2
    );
    storage.commit_writer().unwrap();
    assert_publication(&storage, &expected());
}

#[test]
fn missing_timestamp_evidence_alone_requires_replay_without_changing_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let _upstream = fixture(&path);
    let discovered = discover(&path).unwrap().remove(0);
    let source = discovered.identity;
    let parsed = parse(&source).unwrap().chat.unwrap();
    let local = directory.path().join("silver.sqlite3");
    let mut storage = Storage::open_writer(&local).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    let history =
        serde_json::to_vec(&storage.view(&parsed.messages.inline()[0].id, 10).unwrap()).unwrap();
    let mut legacy = expected();
    legacy.timestamps = None;
    storage.commit_writer().unwrap();
    let connection = Connection::open(&local).unwrap();
    connection
        .execute(
            "UPDATE chat_enrichment SET evidence=?1 WHERE chat_id=?2",
            rusqlite::params![serde_json::to_string(&legacy).unwrap(), parsed.id],
        )
        .unwrap();
    drop(connection);
    let mut stamp = source_stamp_for(&source).unwrap();
    stamp.activity_ns = discovered.activity_ns;
    assert!(
        !storage
            .source_checkpoint_matches(Provider::Opencode, &source, &stamp)
            .unwrap()
    );
    storage
        .with_publication_snapshot(|snapshot| {
            snapshot.visit(crate::shared::protocol::BatchKind::Chats, |batch| {
                let crate::shared::protocol::Batch::Chats { records } = batch else {
                    panic!("conversation batch")
                };
                assert_eq!(
                    records
                        .iter()
                        .find(|record| record.id == parsed.id)
                        .unwrap()
                        .enrichment
                        .as_ref(),
                    Some(&legacy)
                );
                Ok(())
            })
        })
        .unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&local).unwrap();
    let replay = index(&mut storage, &options(&path)).unwrap();
    assert_eq!(replay.changed_messages, 0);
    assert_eq!(replay.checkpoint_skipped_sources, 1);
    assert_eq!(
        storage.chat_enrichment(&parsed.id).unwrap(),
        Some(expected())
    );
    assert_eq!(
        serde_json::to_vec(&storage.view(&parsed.messages.inline()[0].id, 10).unwrap()).unwrap(),
        history
    );
    assert!(
        storage
            .source_checkpoint_matches(Provider::Opencode, &source, &stamp)
            .unwrap()
    );
}

fn assert_publication(storage: &Storage, expected: &Evidence) {
    storage
        .with_publication_snapshot(|snapshot| {
            snapshot.visit(crate::shared::protocol::BatchKind::Chats, |batch| {
                let crate::shared::protocol::Batch::Chats { records } = batch else {
                    panic!("conversation batch")
                };
                assert!(
                    records
                        .iter()
                        .all(|record| record.enrichment.as_ref() == Some(expected))
                );
                Ok(())
            })
        })
        .unwrap();
}
