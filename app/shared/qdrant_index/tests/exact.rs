use super::*;
use crate::gold::readiness::State;

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
