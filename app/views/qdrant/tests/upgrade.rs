//! Views published while word search ranked from sparse vectors gain the text
//! index in place: no document is republished or re-embedded.
use super::*;

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.20+"]
async fn qdrant_sparse_era_views_upgrade_in_place_without_republishing() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "durable"))
        .await
        .unwrap();
    assert_eq!(fixture.search().await.lexical.len(), 2);
    let before = fixture.server.state(&fixture.identity).await.unwrap();

    // Put the group back as a schema-12 release left it: certified documents,
    // no text index, and a configuration that predates it.
    let http = reqwest::Client::new();
    let messages = collection(&fixture.identity.group_id, false);
    http.delete(format!(
        "{}/collections/{messages}/index/unit.text?wait=true",
        fixture.url
    ))
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap();
    http.post(format!(
        "{}/collections/{}/points/payload?wait=true",
        fixture.url,
        collection(&fixture.identity.group_id, true)
    ))
    .json(&serde_json::json!({
        "payload": {"schema_version": 12},
        "points": [point_id(&["views-configuration"])],
    }))
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap();

    // Readers never repair: they report what to run and serve nothing.
    let unopened = Store::open(
        &fixture.url,
        None,
        std::slice::from_ref(&fixture.identity.group_id),
        fixture.identity.dimensions,
        64 * 1024 * 1024,
        VectorIndex::default(),
        MemoryTier::Cached,
    )
    .await;
    assert!(unopened.is_err());
    let stale = fixture
        .server
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap_err();
    assert!(
        stale.error.message.contains("run `mash index`"),
        "{stale:?}"
    );

    // Explicit setup builds the index from stored payloads and records it.
    let server = fixture
        .reopen(VectorIndex::default(), MemoryTier::Cached)
        .await
        .unwrap();
    let result = server
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    assert_eq!(result.lexical.len(), 2);
    assert_eq!(result.semantic.len(), 2);
    let after = server.state(&fixture.identity).await.unwrap();
    assert_eq!(after, before);
    fixture.cleanup().await;
}
