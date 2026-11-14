use super::*;
use crate::silver::{SourceIdentity, SourceStamp};

#[test]
fn missing_outdated_and_malformed_evidence_cannot_skip_replay_and_deletes_cascade() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("silver.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    let thread = crate::storage::tests::chat_with_messages(&[("message", "session")]);
    storage.replace_chat(&thread).unwrap();
    let stamp = SourceStamp::file(10, 20);
    storage
        .record_source_checkpoint(thread.provider, &thread.source, &stamp)
        .unwrap();
    assert!(
        storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap()
    );
    let evidence = storage.chat_enrichment(&thread.id).unwrap().unwrap();
    assert!(evidence.is_current());
    let before = storage.silver_certificate().unwrap().revision;
    storage
        .connection
        .execute("DELETE FROM chat_enrichment", [])
        .unwrap();
    assert!(storage.silver_certificate().unwrap().revision > before);
    assert!(
        !storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap()
    );
    assert!(storage.replace_chat(&thread).unwrap().unchanged);
    assert_eq!(
        storage.chat_enrichment(&thread.id).unwrap(),
        Some(evidence.clone())
    );
    let mut outdated = evidence;
    outdated.version += 1;
    storage.store_enrichment(&thread.id, &outdated).unwrap();
    assert!(
        !storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap()
    );
    storage
        .connection
        .execute("UPDATE chat_enrichment SET evidence='{}'", [])
        .unwrap();
    assert_eq!(
        storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap_err()
            .error
            .kind,
        crate::ErrorKind::Database
    );
    storage.replace_chat(&thread).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let mut storage = Storage::open_existing(&path).unwrap();
    assert!(storage.forget(&thread.id).unwrap());
    assert!(storage.chat_enrichment(&thread.id).unwrap().is_none());
    assert!(
        storage
            .source_enrichment_is_current(thread.provider, &SourceIdentity::file("empty").key())
            .unwrap()
    );
}
