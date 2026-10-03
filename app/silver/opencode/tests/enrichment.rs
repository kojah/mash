use super::*;
use crate::silver::enrichment::{Evidence, Origin, SourceField};

fn expected() -> Evidence {
    Evidence {
        version: crate::silver::enrichment::VERSION,
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
    let parsed = parse(&source).unwrap().conversation.unwrap();
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
    let parsed = parse(&source).unwrap().conversation.unwrap();
    assert_eq!(Evidence::capture(&parsed).unwrap(), expected());
    let local = directory.path().join("silver.sqlite3");
    let mut storage = Storage::open_writer(&local).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    let history =
        serde_json::to_vec(&storage.view(&parsed.messages.inline()[0].id, 10).unwrap()).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let stored = Storage::open_catalog(&local).unwrap();
    assert_eq!(
        stored.conversation_enrichment(&parsed.id).unwrap(),
        Some(expected())
    );
    assert!(
        stored
            .thread_candidates(&crate::thread_catalog::Request::default(), "local")
            .unwrap()
            .iter()
            .all(|thread| thread.metadata.enrichment == Some(expected()))
    );
    let mut old = expected();
    old.title = Some(Origin::Unknown);
    old.project = Some(Origin::Unknown);
    old.repository = Some(Origin::Unknown);
    let connection = Connection::open(&local).unwrap();
    connection
        .execute(
            "UPDATE conversation_enrichment SET evidence=?1",
            [serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
    drop(connection);
    assert_eq!(
        stored.conversation_enrichment(&parsed.id).unwrap(),
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
        storage.conversation_enrichment(&parsed.id).unwrap(),
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

fn assert_publication(storage: &Storage, expected: &Evidence) {
    storage
        .with_publication_snapshot(|snapshot| {
            snapshot.visit(crate::shared::protocol::BatchKind::Conversations, |batch| {
                let crate::shared::protocol::Batch::Conversations { records } = batch else {
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
