use super::*;
use crate::gold::readiness::State;

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_flexible_lexical_search_matches_prefixes_and_typos() {
    use crate::gold::matching::{Kind, Mode};
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "idempotency redis4"))
        .await
        .unwrap();
    for (text, kind) in [("idemp", Kind::Prefix), ("idempotnecy", Kind::Typo)] {
        let mut input = exact_query(&fixture, text);
        input.match_mode = Mode::Flexible;
        let result = fixture
            .server
            .search(&fixture.identity, input)
            .await
            .unwrap();
        assert_eq!(result.lexical.len(), 2, "{text}");
        assert!(
            result
                .lexical
                .iter()
                .all(|h| h.lexical_match.as_ref().is_some_and(|e| e.kind == kind))
        );
        let mut input = fixture.query();
        input.query = text.into();
        input.match_mode = Mode::Flexible;
        let result = fixture
            .server
            .search(&fixture.identity, input)
            .await
            .unwrap();
        assert_eq!(result.lexical.len(), 2, "hybrid lexical {text}");
        assert!(
            result
                .lexical
                .iter()
                .all(|h| h.lexical_match.as_ref().is_some_and(|e| e.kind == kind))
        );
        assert_eq!(lexical(&fixture, &fixture.server, text).await, 0);
    }
    for text in ["redis40", "\"idemp\""] {
        let mut input = exact_query(&fixture, text);
        input.match_mode = Mode::Flexible;
        assert!(
            fixture
                .server
                .search(&fixture.identity, input)
                .await
                .unwrap()
                .lexical
                .is_empty()
        );
    }
    let mut input = exact_query(&fixture, "idemp");
    input.match_mode = Mode::Flexible;
    input.filter = Some(crate::search_filter::Filter::Conversation(vec![
        "absent".into(),
    ]));
    assert!(
        fixture
            .server
            .search(&fixture.identity, input)
            .await
            .unwrap()
            .lexical
            .is_empty()
    );
    let mut messages = fixture.messages("idempotency");
    messages[0].content = "idemp".into();
    messages[0].projection = "idemp".into();
    let file = Fixture::upload(&[
        Event::Start(Fixture::start(2)),
        Event::Batch(Batch::Conversations {
            records: vec![Fixture::conversation()],
        }),
        Event::Batch(Batch::Messages { records: messages }),
        Event::Finish,
    ]);
    fixture.server.index(&fixture.identity, file).await.unwrap();
    let mut input = fixture.query();
    input.query = "idemp".into();
    input.match_mode = Mode::Flexible;
    let hits = fixture
        .server
        .search(&fixture.identity, input)
        .await
        .unwrap()
        .lexical;
    assert_eq!(hits[0].id, "message-0", "exact before expanded terms");
    assert!(hits[0].lexical_match.is_none());
    let mut input = exact_query(&fixture, "idemp");
    input.match_mode = Mode::Flexible;
    let hits = fixture
        .server
        .search(&fixture.identity, input)
        .await
        .unwrap()
        .lexical;
    assert_eq!(hits[0].id, "message-0", "exact before newer prefix");
    fixture.cleanup().await;
}

fn exact_query(fixture: &Fixture, text: &str) -> protocol::Search {
    let mut query = fixture.query();
    query.intent = crate::gold::exact::Intent::ExactMention;
    query.query = text.into();
    query.vector.clear();
    query.generation.clear();
    query
}

async fn lexical(fixture: &Fixture, server: &Server, text: &str) -> usize {
    server
        .search(&fixture.identity, exact_query(fixture, text))
        .await
        .unwrap()
        .lexical
        .len()
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_exact_search_follows_silver_and_survives_recipe_changes() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    assert!(
        fixture
            .server
            .freshness(&fixture.identity)
            .await
            .unwrap()
            .nodes
            .is_empty()
    );
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "retained"))
        .await
        .unwrap();
    assert_eq!(lexical(&fixture, &fixture.server, "retained").await, 2);
    // Recipe changes affect dense retrieval, not lexical documents.
    let other = fixture
        .store
        .clone()
        .with_build(crate::gold::config::Build {
            max_embedding_chars: None,
            representations: crate::gold::config::Representations::LexicalAndDense,
            dense_content: crate::gold::config::DenseContent::AllVisible,
            retrieval_unit: crate::gold::config::RetrievalUnit::Message,
            text_preparation: crate::gold::config::TextPreparation::Preserve,
            embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
        });
    other.select_gold(&fixture.identity, true).await.unwrap();
    let switched = fixture.serve(other);
    assert_eq!(lexical(&fixture, &switched, "retained").await, 2);
    let artifacts = switched.freshness(&fixture.identity).await.unwrap().nodes[0]
        .artifacts
        .clone()
        .unwrap();
    assert_eq!(artifacts.lexical, State::Ready);
    assert_eq!(artifacts.dense, State::Stale);
    assert!(
        switched
            .search(&fixture.identity, fixture.query())
            .await
            .is_err(),
        "dense search fails closed for incompatible gold"
    );
    fixture
        .store
        .select_gold(&fixture.identity, true)
        .await
        .unwrap();
    // Edits replace documents in place; old text is no longer searchable.
    fixture
        .server
        .index(&fixture.identity, fixture.file(2, "changed"))
        .await
        .unwrap();
    assert_eq!(lexical(&fixture, &fixture.server, "changed").await, 2);
    assert_eq!(lexical(&fixture, &fixture.server, "retained").await, 0);
    let freshness = fixture.server.freshness(&fixture.identity).await.unwrap();
    assert!(freshness.nodes[0].lexical_current);
    assert!(freshness.nodes[0].gold_current);
    fixture.cleanup().await;
}
