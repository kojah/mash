use super::*;
use crate::{
    message_context::MessageContext,
    provider::Provider,
    role::Role,
    silver::archive::{Record, ScanState},
};
use std::io::{Read, Write};

fn history(content: &str, revision: u64) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for record in [
        Record::Conversation {
            id: "thread".into(),
            provider: Provider::Codex,
            source_path: "/not-read/history.jsonl".into(),
            source_key: "file:/not-read/history.jsonl".into(),
            source_session: None,
            title: Some("A thread".into()),
            created_at: Some(1),
            updated_at: Some(2),
            source_fingerprint: "fingerprint".into(),
            project: None,
            repository: None,
        },
        Record::Message {
            attachments: vec![],
            id: "message".into(),
            conversation_id: "thread".into(),
            ordinal: 0,
            role: Role::User,
            content: content.into(),
            dialogue_text: None,
            context: MessageContext::Root,
            created_at: Some(1),
            fingerprint: "message-fingerprint".into(),
        },
        Record::Scan {
            provider: Provider::Codex,
            interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
            state: ScanState::Complete,
        },
        Record::State {
            revision,
            interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
        },
    ] {
        serde_json::to_writer(file.as_file_mut(), &record).unwrap();
        writeln!(file).unwrap();
    }
    file
}

fn fixture() -> Store {
    let mut random = [0; 16];
    getrandom::fill(&mut random).unwrap();
    Store::new(
        &std::env::var("MASH_TEST_QDRANT_URL").unwrap(),
        None,
        &format!("canonical-{}", hex::encode(random)),
        32 * 1024 * 1024,
    )
    .unwrap()
}

async fn cleanup(store: &Store) {
    let url = std::env::var("MASH_TEST_QDRANT_URL").unwrap();
    reqwest::Client::new()
        .delete(format!("{url}/collections/{}", store.collection))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
}

fn contents(file: &tempfile::NamedTempFile) -> Vec<u8> {
    let mut bytes = Vec::new();
    file.reopen().unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn canonical_qdrant_chunk_boundary_preserves_a_split_four_byte_character() {
    let store = fixture();
    store.initialize().await.unwrap();
    let example = contents(&history("UNIQUE_MARKER", 1));
    let prefix = example
        .windows(b"UNIQUE_MARKER".len())
        .position(|window| window == b"UNIQUE_MARKER")
        .unwrap();
    let text = format!("{}🙂suffix", "x".repeat(256 * 1024 - 1 - prefix));
    let original = history(&text, 1);
    let bytes = contents(&original);
    assert_eq!(
        bytes[256 * 1024 - 1],
        0xf0,
        "place the first emoji byte at the chunk boundary"
    );
    let head = store
        .publish(
            Verified::new(original, store.max_bytes).await.unwrap(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(contents(&store.read(&head).await.unwrap()), bytes);
    cleanup(&store).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn canonical_qdrant_import_reopens_without_sqlite_and_keeps_captured_revisions_readable() {
    let store = fixture();
    assert!(store.head().await.unwrap().is_none());
    assert!(
        !store.api.metadata_exists(&store.collection).await.unwrap(),
        "read did not create a collection"
    );
    store.initialize().await.unwrap();
    let original = history(&"Unicode αβγ🙂 and escaped \"quotes\"\n".repeat(20_000), 1);
    let expected_bytes = contents(&original);
    let first = store
        .publish(
            Verified::new(original, store.max_bytes).await.unwrap(),
            None,
        )
        .await
        .unwrap();
    assert!(first.chunks > 1, "exercise UTF-8 chunk boundaries");
    assert_eq!(first.summary.counts.messages, 1);
    assert_eq!(first.summary.counts.embeddings, 0);
    let reopened = Store::new(
        &std::env::var("MASH_TEST_QDRANT_URL").unwrap(),
        None,
        "unused",
        store.max_bytes,
    )
    .unwrap();
    // Reopen the exact namespace with a fresh transport, without initialization.
    let reopened = Store {
        collection: store.collection.clone(),
        scope: store.scope.clone(),
        ..reopened
    };
    assert_eq!(
        reopened.head().await.unwrap().unwrap().version,
        first.version
    );
    assert_eq!(
        contents(&reopened.read(&first).await.unwrap()),
        expected_bytes
    );
    let unchanged = reopened
        .publish(
            Verified::new(
                history(&"Unicode αβγ🙂 and escaped \"quotes\"\n".repeat(20_000), 1),
                store.max_bytes,
            )
            .await
            .unwrap(),
            Some(&first.version),
        )
        .await
        .unwrap();
    assert_eq!(
        unchanged.version, first.version,
        "unchanged data reuses its immutable publication"
    );
    assert_eq!(unchanged.last_updated_at, first.last_updated_at);
    let next = history("changed", 2);
    let next_bytes = contents(&next);
    let second = reopened
        .publish(
            Verified::new(next, store.max_bytes).await.unwrap(),
            Some(&first.version),
        )
        .await
        .unwrap();
    assert_eq!(contents(&reopened.read(&second).await.unwrap()), next_bytes);
    assert_eq!(
        contents(&reopened.read(&first).await.unwrap()),
        expected_bytes,
        "new publication did not remove captured immutable data"
    );
    let rejected = reopened
        .publish(
            Verified::new(history("stale", 3), store.max_bytes)
                .await
                .unwrap(),
            Some(&first.version),
        )
        .await
        .unwrap_err();
    assert_eq!(rejected.error.kind, crate::ErrorKind::Conflict);
    assert_eq!(
        reopened.head().await.unwrap().unwrap().version,
        second.version
    );
    cleanup(&store).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn canonical_qdrant_concurrent_writers_cannot_overwrite_the_winning_revision() {
    let store = fixture();
    store.initialize().await.unwrap();
    let first = store
        .publish(
            Verified::new(history("base", 1), store.max_bytes)
                .await
                .unwrap(),
            None,
        )
        .await
        .unwrap();
    let a = Verified::new(history("writer-a", 2), store.max_bytes)
        .await
        .unwrap();
    let b = Verified::new(history("writer-b", 2), store.max_bytes)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        store.publish(a, Some(&first.version)),
        store.publish(b, Some(&first.version))
    );
    assert_ne!(
        a.is_ok(),
        b.is_ok(),
        "exactly one conditional publication wins"
    );
    let ((Ok(winner), Err(loser)) | (Err(loser), Ok(winner))) = (a, b) else {
        unreachable!()
    };
    assert_eq!(loser.error.kind, crate::ErrorKind::Conflict);
    assert_eq!(store.head().await.unwrap().unwrap().version, winner.version);
    assert!(store.read(&winner).await.is_ok());
    cleanup(&store).await;
}

#[tokio::test]
async fn canonical_validation_rejects_bad_relationships_before_any_backend_write() {
    let mut file = history("valid", 1);
    let mut body = String::from_utf8(contents(&file)).unwrap();
    body = body.replace(
        "\"conversation_id\":\"thread\"",
        "\"conversation_id\":\"missing\"",
    );
    file.as_file_mut().set_len(0).unwrap();
    std::io::Seek::rewind(file.as_file_mut()).unwrap();
    file.write_all(body.as_bytes()).unwrap();
    assert!(Verified::new(file, 32 * 1024 * 1024).await.is_err());
}

#[tokio::test]
#[ignore = "requires an isolated MASH_TEST_QDRANT_URL and private MASH_TEST_CANONICAL_ARCHIVE"]
async fn canonical_qdrant_real_archive_roundtrip() {
    let path =
        std::env::var("MASH_TEST_CANONICAL_ARCHIVE").expect("private canonical archive path");
    let input = crate::blocking(move || {
        let mut file = tempfile::NamedTempFile::new().map_err(AppError::io)?;
        std::io::copy(
            &mut std::fs::File::open(path).map_err(AppError::io)?,
            file.as_file_mut(),
        )
        .map_err(AppError::io)?;
        Ok(file)
    })
    .await
    .unwrap();
    let store = Store {
        max_bytes: input.as_file().metadata().unwrap().len().saturating_mul(2),
        ..fixture()
    };
    let verified = Verified::new(input, store.max_bytes).await.unwrap();
    let original = verified.summary.clone();
    let started = std::time::Instant::now();
    store.initialize().await.unwrap();
    let head = store.publish(verified, None).await.unwrap();
    let published_ms = started.elapsed().as_millis();
    let started = std::time::Instant::now();
    let restored = store.read(&head).await.unwrap();
    let read_ms = started.elapsed().as_millis();
    let recovered = Verified::new(restored, store.max_bytes).await.unwrap();
    assert_eq!(
        recovered.summary, original,
        "all canonical records, relationships, certificate and bytes survived Qdrant"
    );
    assert_eq!(store.head().await.unwrap().unwrap().version, head.version);
    eprintln!(
        "canonical_qdrant_roundtrip bytes={} threads={} messages={} chunks={} publish_ms={published_ms} read_ms={read_ms}",
        original.bytes, original.counts.conversations, original.counts.messages, head.chunks
    );
    cleanup(&store).await;
}
