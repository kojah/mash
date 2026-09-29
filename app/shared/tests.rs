use super::*;
use crate::ingestion::{Conversation, NormalizedMessage};
use crate::storage::EmbeddingWrite;

fn seed(path: &Path) -> Storage {
    let mut storage = Storage::open_writer(path).expect("writer");
    storage
        .replace_conversation(&Conversation {
            id: "conversation".to_owned(),
            provider: "codex",
            source_path: "/tmp/shared-fixture.jsonl".into(),
            title: Some("fixture".to_owned()),
            created_at: Some(1),
            updated_at: Some(2),
            messages: vec![
                NormalizedMessage {
                    id: "searchable".to_owned(),
                    ordinal: 0,
                    role: "user".to_owned(),
                    content: "hello shared history".to_owned(),
                    search_projection: None,
                    created_at: Some(1),
                },
                NormalizedMessage {
                    id: "context".to_owned(),
                    ordinal: 1,
                    role: "tool".to_owned(),
                    content: "hello raw tool output".to_owned(),
                    search_projection: Some(String::new()),
                    created_at: Some(2),
                },
            ],
        })
        .expect("conversation");
    storage
        .replace_embeddings(
            semantic::embedding_generation(),
            &[EmbeddingWrite {
                message_id: "searchable",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("embedding");
    storage.commit_writer().expect("commit");
    storage
}

#[test]
fn local_candidates_use_the_shared_query_vector_and_keep_context_out_of_search() {
    let dir = tempfile::tempdir().unwrap();
    let storage = seed(&dir.path().join("local.sqlite3"));
    let (hits, documents) = semantic::local_candidates(
        &storage,
        semantic::embedding_generation(),
        "hello",
        &[127.0, 0.0],
        None,
        None,
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "searchable");
    assert_eq!(hits[0].node_id, None);
    assert!(hits[0].semantic_score.unwrap() > 0.99);
    assert_eq!(documents, ["hello shared history"]);
    assert!(
        semantic::local_candidates(
            &storage,
            semantic::embedding_generation(),
            "hello",
            &[127.0, 0.0],
            Some("claude-code"),
            None
        )
        .unwrap()
        .0
        .is_empty()
    );
}

#[test]
fn routed_forget_acknowledgements_preserve_newer_intents_and_pending_indexing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local.sqlite3");
    let storage = seed(&path);
    storage.queue_shared_forget("group-node", "first").unwrap();
    let first = storage.direct_owner("group-node").unwrap();
    storage.queue_shared_forget("group-node", "second").unwrap();
    storage.acknowledge_direct("routed", &first, true).unwrap();
    let pending = storage.direct_owner("group-node").unwrap();
    assert_eq!(pending.forgets, ["first", "second"]);
    storage
        .acknowledge_direct("routed", &pending, false)
        .unwrap();
    assert!(
        Storage::publication_is_pending(&path, "routed").unwrap(),
        "deletion does not acknowledge other unpublished index changes"
    );
    let indexed = storage.direct_owner("group-node").unwrap();
    assert_eq!(indexed.forgets, Vec::<String>::new());
    storage
        .acknowledge_direct("routed", &indexed, true)
        .unwrap();
    assert!(!Storage::publication_is_pending(&path, "routed").unwrap());
}

#[tokio::test]
async fn publication_state_survives_failed_delivery_and_forget() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("local.sqlite3");
    let mut storage = seed(&path);
    let unavailable = SharedConfig {
        group_id: "test".to_owned(),
        node_id: "test".to_owned(),
        api_url: "http://127.0.0.1:9".to_owned(),
        token_env: "MASH_TEST_UNCONFIGURED_TOKEN_71B9".to_owned(),
    };
    let delivery = publish(&path, &unavailable).await;
    assert_eq!(delivery.state, "pending");
    assert!(delivery.error.is_some());
    assert!(Storage::publication_is_pending(&path, "target").expect("pending"));
    let revision = storage
        .with_publication_snapshot(|snapshot| {
            let mut messages = Vec::new();
            snapshot.visit("messages", |batch| {
                messages.extend_from_slice(batch);
                Ok(())
            })?;
            assert_eq!(messages.len(), 2);
            let tool = messages
                .iter()
                .find(|m| m["id"] == "context")
                .expect("tool");
            assert_eq!(tool["projection"], "");
            assert_eq!(tool["content"], "hello raw tool output");
            Ok(())
        })
        .expect("snapshot");
    storage
        .acknowledge_publication("target", revision)
        .expect("ack");
    assert!(!Storage::publication_is_pending(&path, "target").expect("acked"));
    assert!(
        Storage::publication_is_pending(&path, "different destination").expect("target change")
    );
    storage.forget("conversation").expect("forget");
    assert!(Storage::publication_is_pending(&path, "target").expect("pending delete"));
    storage
        .acknowledge_publication("target", revision)
        .expect("stale ack ignored");
    assert!(Storage::publication_is_pending(&path, "target").expect("still pending"));
    storage
        .with_publication_snapshot(|snapshot| {
            snapshot.visit("tombstones", |batch| {
                assert_eq!(batch[0]["conversation_id"], "conversation");
                Ok(())
            })
        })
        .expect("durable deletion");
}
