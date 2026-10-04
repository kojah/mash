use super::*;

fn two_threads(
    fixture: &Fixture,
    revision: i64,
    second_text: &str,
    keep_first: bool,
) -> tempfile::NamedTempFile {
    use std::io::BufRead;
    let input = fixture.file(revision, "unchanged");
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for line in std::io::BufReader::new(input.reopen().unwrap()).lines() {
        let mut event: Event = serde_json::from_str(&line.unwrap()).unwrap();
        match &mut event {
            Event::Batch(Batch::Conversations { records }) => {
                let mut second = records[0].clone();
                second.id = "second".into();
                if !keep_first {
                    records.clear();
                }
                records.push(second);
            }
            Event::Batch(Batch::Messages { records }) => {
                let mut second = records.clone();
                for message in &mut second {
                    message.id = format!("second-{}", message.id);
                    message.conversation_id = "second".into();
                    message.content = format!("needle canonical {second_text}");
                    message.projection = format!("needle {second_text}");
                }
                if !keep_first {
                    records.clear();
                }
                records.extend(second);
            }
            _ => {}
        }
        writeln!(file, "{}", serde_json::to_string(&event).unwrap()).unwrap();
    }
    file
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_incremental_threads_retain_unchanged_points_and_reclaim_edits_and_deletions() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(
            &fixture.identity,
            two_threads(&fixture, 1, "original", true),
        )
        .await
        .unwrap();
    let first = fixture.store.node(&fixture.identity).await.unwrap();
    fixture
        .store
        .index(&fixture.identity, two_threads(&fixture, 2, "edited", true))
        .await
        .unwrap();
    let edited = fixture.store.node(&fixture.identity).await.unwrap();
    assert_eq!(
        first.thread_version("thread"),
        edited.thread_version("thread")
    );
    assert_ne!(
        first.thread_version("second"),
        edited.thread_version("second")
    );
    let retired = point_id(&[
        "message",
        first.thread_version("second").unwrap(),
        "second-message-0",
    ]);
    assert!(
        fixture
            .store
            .api
            .retrieve::<Document>(&collection(&fixture.identity.group_id, false), &[retired])
            .await
            .unwrap()
            .is_empty()
    );
    let loaded = fixture.store.load(&fixture.identity).await.unwrap();
    assert_eq!(loaded.data.conversations.len(), 2);
    assert_eq!(loaded.data.messages.len(), 4);
    assert_eq!(
        fixture
            .store
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap()
            .semantic
            .len(),
        4
    );
    fixture
        .store
        .index(&fixture.identity, two_threads(&fixture, 3, "edited", false))
        .await
        .unwrap();
    let deleted = fixture.store.node(&fixture.identity).await.unwrap();
    assert_eq!(
        edited.thread_version("second"),
        deleted.thread_version("second")
    );
    assert_eq!(deleted.thread_version("thread"), None);
    assert_eq!(
        fixture
            .store
            .load(&fixture.identity)
            .await
            .unwrap()
            .data
            .messages
            .len(),
        2
    );
    assert_eq!(
        fixture
            .store
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap()
            .semantic
            .len(),
        2
    );
    fixture.cleanup().await;
}
