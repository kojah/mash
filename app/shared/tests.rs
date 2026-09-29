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
        database_url_env: "MASH_TEST_UNCONFIGURED_DATABASE_71B9".to_owned(),
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

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL pointing to an expendable PostgreSQL database"]
#[allow(clippy::too_many_lines)]
async fn postgres_publication_groups_search_context_retry_and_deletion() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("local.sqlite3");
    let mut storage = seed(&path);
    let mut config = SharedConfig {
        group_id: format!(
            "test-{}",
            dir.path().file_name().expect("name").to_string_lossy()
        ),
        node_id: "first".to_owned(),
        database_url_env: "MASH_TEST_PG_URL".to_owned(),
    };
    publish_result(&path, &config).await.expect("publish");
    publish_result(&path, &config)
        .await
        .expect("retry lost acknowledgement is idempotent");
    assert_eq!(
        status(&path, Some(&config))
            .expect("status")
            .expect("configured")
            .state,
        "published"
    );
    let (mut client, _) = connection(&config).await.expect("client");
    let Candidates {
        lexical,
        semantic: semantic_hits,
        ..
    } = candidates(&mut client, "hello", &[127.0, 0.0], None, None)
        .await
        .expect("search");
    assert_eq!(lexical.len(), 1);
    assert_eq!(semantic_hits.len(), 1);
    assert_eq!(semantic_hits[0].semantic_score, Some(1.0));
    assert_eq!(
        view(&config, "first", "searchable", 1)
            .await
            .expect("offline source view")
            .len(),
        2
    );
    config.node_id = "second".to_owned();
    publish_result(&path, &config)
        .await
        .expect("second publisher");
    let Candidates {
        lexical,
        semantic: semantic_hits,
        ..
    } = candidates(&mut client, "hello", &[127.0, 0.0], None, None)
        .await
        .expect("joint search");
    assert_eq!(
        semantic::fuse(lexical, semantic_hits).len(),
        2,
        "same message ID on different owners stays distinct"
    );
    let original_group = config.group_id.clone();
    config.group_id.push_str("-separate");
    publish_result(&path, &config).await.expect("second group");
    assert!(
        view(&config, "first", "searchable", 0)
            .await
            .expect("group isolation")
            .is_empty()
    );
    config.group_id = original_group;
    config.node_id = "first".to_owned();
    // A different local DB must not overwrite an existing publisher identity.
    let another_path = dir.path().join("another.sqlite3");
    let _another = seed(&another_path);
    assert!(publish_result(&another_path, &config).await.is_err());
    // Reject stale revisions before deleting or replacing shared records.
    client
        .execute(
            "UPDATE nodes SET revision=revision+100 WHERE node_id='first'",
            &[],
        )
        .await
        .expect("advance");
    assert!(publish_result(&path, &config).await.is_err());
    assert_eq!(
        view(&config, "first", "searchable", 0)
            .await
            .expect("not overwritten")
            .len(),
        1
    );
    client
        .execute(
            "UPDATE nodes SET revision=revision-100 WHERE node_id='first'",
            &[],
        )
        .await
        .expect("restore");
    let backup_path = dir.path().join("backup.sqlite3");
    rusqlite::Connection::open(&path)
        .expect("backup source")
        .execute("VACUUM INTO ?1", [backup_path.to_str().expect("path")])
        .expect("consistent backup");
    let backup = Storage::open(&backup_path).expect("restored snapshot");
    storage.forget("conversation").expect("local deletion");
    publish_result(&path, &config)
        .await
        .expect("publish deletion");
    assert!(
        view(&config, "first", "searchable", 1)
            .await
            .expect("deleted")
            .is_empty()
    );
    assert_eq!(
        view(&config, "second", "searchable", 1)
            .await
            .expect("other owner retained")
            .len(),
        2
    );
    // A stale backup cannot resurrect an acknowledged tombstone.
    client
        .execute("UPDATE nodes SET revision=0 WHERE node_id='first'", &[])
        .await
        .expect("force replay test");
    // Replays of the same instance still preserve shared tombstones.
    publish_result(&backup_path, &config)
        .await
        .expect("replay pre-deletion snapshot");
    assert!(
        view(&config, "first", "searchable", 0)
            .await
            .expect("still deleted")
            .is_empty()
    );
    drop(backup);
    client
        .batch_execute(&format!("DROP SCHEMA {} CASCADE", group_schema(&config)))
        .await
        .expect("cleanup");
    config.group_id.push_str("-separate");
    client
        .batch_execute(&format!("DROP SCHEMA {} CASCADE", group_schema(&config)))
        .await
        .expect("cleanup separate");
}
