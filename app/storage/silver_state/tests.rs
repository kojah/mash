use super::*;
use crate::storage::{EmbeddingWrite, Storage};

#[test]
fn rejected_refresh_can_resume_only_the_unchanged_previously_published_gold() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("history.sqlite3")).unwrap();
    let mut conversation =
        crate::storage::tests::conversation_with_messages(&[("message-1", "qdrant vectors")]);
    storage.replace_conversation(&conversation).unwrap();
    storage.begin_silver_scan(Provider::Codex).unwrap();
    storage.finish_silver_scan(Provider::Codex, true).unwrap();
    storage.finish_silver_ingestion().unwrap();
    storage.rebuild_derived_search_state().unwrap();
    storage
        .replace_embeddings(
            "fixture",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .unwrap();
    storage.mark_semantic_index_ready("fixture").unwrap();
    storage.begin_silver_scan(Provider::Codex).unwrap();
    storage
        .reject_silver_source(
            Provider::Codex,
            &conversation.source,
            &SourceStamp::file(10, 20),
        )
        .unwrap();
    storage.finish_silver_scan(Provider::Codex, false).unwrap();
    assert!(!storage.semantic_index_is_ready("fixture").unwrap());
    storage
        .resume_unchanged_semantic_snapshot("other-model")
        .unwrap();
    assert!(!storage.semantic_index_is_ready("fixture").unwrap());
    storage
        .resume_unchanged_semantic_snapshot("fixture")
        .unwrap();
    assert!(storage.semantic_index_is_ready("fixture").unwrap());
    assert!(!storage.silver_certificate().unwrap().is_complete());
    storage.begin_silver_scan(Provider::Codex).unwrap();
    conversation.title = Some("changed canonical metadata".into());
    storage.replace_conversation(&conversation).unwrap();
    storage
        .resume_unchanged_semantic_snapshot("fixture")
        .unwrap();
    assert!(!storage.semantic_index_is_ready("fixture").unwrap());
}

#[test]
fn canonical_revision_excludes_gold_and_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    assert_eq!(
        storage.silver_certificate().unwrap().completeness,
        Completeness::Unverified
    );
    let conversation =
        crate::storage::tests::conversation_with_messages(&[("message-1", "qdrant vectors")]);
    storage.replace_conversation(&conversation).unwrap();
    storage.begin_silver_scan(Provider::Codex).unwrap();
    storage.finish_silver_scan(Provider::Codex, true).unwrap();
    storage.finish_silver_ingestion().unwrap();
    let canonical = storage.silver_certificate().unwrap();
    assert!(canonical.is_complete());
    assert!(canonical.revision >= 2);
    storage.rebuild_derived_search_state().unwrap();
    storage
        .replace_embeddings(
            "fixture",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .unwrap();
    storage.mark_semantic_index_ready("fixture").unwrap();
    assert_eq!(storage.silver_certificate().unwrap(), canonical);
    storage.commit_writer().unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&path).unwrap();
    assert_eq!(storage.silver_certificate().unwrap(), canonical);
    let mut changed = conversation;
    changed.title = Some("new title".into());
    storage.replace_conversation(&changed).unwrap();
    assert!(storage.silver_certificate().unwrap().revision > canonical.revision);
    let before = storage.silver_certificate().unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    assert_eq!(
        Storage::open_catalog(&path)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        before
    );
}

#[test]
fn pending_scan_and_rejections_survive_partial_commits_and_other_provider_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.sqlite3");
    let source = SourceIdentity::file(directory.path().join("bad.jsonl"));
    let stamp = SourceStamp::file(10, 20);
    let mut storage = Storage::open_writer(&path).unwrap();
    storage.begin_silver_scan(Provider::Codex).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&path).unwrap();
    assert_eq!(
        storage.silver_certificate().unwrap().completeness,
        Completeness::Pending
    );
    storage
        .reject_silver_source(Provider::Codex, &source, &stamp)
        .unwrap();
    storage.finish_silver_scan(Provider::Codex, false).unwrap();
    storage.begin_silver_scan(Provider::ClaudeCode).unwrap();
    storage
        .finish_silver_scan(Provider::ClaudeCode, true)
        .unwrap();
    storage.finish_silver_ingestion().unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&path).unwrap();
    let certificate = storage.silver_certificate().unwrap();
    assert_eq!(certificate.completeness, Completeness::Incomplete);
    assert_eq!(certificate.rejected_sources, 1);
    // A clean scan cannot erase a rejection that was outside its scope.
    storage.begin_silver_scan(Provider::Codex).unwrap();
    storage.finish_silver_scan(Provider::Codex, true).unwrap();
    assert!(!storage.silver_certificate().unwrap().is_complete());
    storage
        .record_source_checkpoint(Provider::ClaudeCode, &source, &stamp)
        .unwrap();
    assert_eq!(storage.silver_certificate().unwrap().rejected_sources, 1);
    storage
        .record_source_checkpoint(Provider::Codex, &source, &stamp)
        .unwrap();
    assert!(storage.silver_certificate().unwrap().is_complete());
}

#[test]
fn missing_rejections_are_removed_only_with_verified_reconciliation_scope() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("history.sqlite3")).unwrap();
    let source = SourceIdentity::file(directory.path().join("bad.jsonl"));
    storage
        .reject_silver_source(Provider::Codex, &source, &SourceStamp::file(10, 20))
        .unwrap();
    let roots = [directory.path().to_path_buf()];
    let observed = BTreeSet::from([source.key()]);
    storage
        .purge_missing_silver_rejections(Provider::Codex, &observed, &roots, None)
        .unwrap();
    assert_eq!(storage.silver_certificate().unwrap().rejected_sources, 1);
    for (roots, cutoff) in [
        (vec![directory.path().join("elsewhere")], None),
        (roots.to_vec(), Some(21)),
    ] {
        storage
            .purge_missing_silver_rejections(Provider::Codex, &BTreeSet::new(), &roots, cutoff)
            .unwrap();
        assert_eq!(storage.silver_certificate().unwrap().rejected_sources, 1);
    }
    storage
        .purge_missing_silver_rejections(Provider::Codex, &BTreeSet::new(), &roots, None)
        .unwrap();
    assert_eq!(storage.silver_certificate().unwrap().rejected_sources, 0);
}
