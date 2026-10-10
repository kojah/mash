//! Native IDE direct ingestion through the authenticated shared API and both stores.
use super::*;
use crate::shared::protocol::{Candidates, Search, View};
use crate::{
    provider::Provider,
    silver::{discovered_sources, parse_test_source},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    rows: Vec<Row>,
}
#[derive(Deserialize)]
struct Row {
    key: String,
    value: String,
}

fn native_publication() -> (Vec<Value>, Vec<String>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.vscdb");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA user_version=1; PRAGMA journal_mode=WAL; CREATE TABLE ItemTable(key TEXT UNIQUE ON CONFLICT REPLACE,value BLOB); CREATE TABLE cursorDiskKV(key TEXT UNIQUE ON CONFLICT REPLACE,value BLOB);").unwrap();
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../../fixtures/cursor-ide-native-v18.json"
    ))
    .unwrap();
    for row in fixture.rows {
        connection
            .execute(
                "INSERT INTO cursorDiskKV VALUES(?1,?2)",
                rusqlite::params![row.key, row.value],
            )
            .unwrap();
    }
    let roots = vec![path.clone()];
    let sources = discovered_sources(Provider::Cursor, &roots);
    let source = &sources[0];
    let conversation = parse_test_source(Provider::Cursor, source)
        .conversation
        .unwrap();
    super::native::publication(conversation)
}

async fn contract(service: Service) {
    let (events, ids) = native_publication();
    assert_eq!(ids.len(), 6);
    let response = service.upload(FIRST, &events).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().await.unwrap()
    );
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    for (query, lexical) in [("cbioportal", 2), ("private", 0)] {
        let input = Search {
            match_mode: crate::views::matching::Mode::Strict,
            intent: crate::views::exact::Intent::RelatedDiscussion,
            vector_policy: None,
            unit: crate::search_results::SearchUnit::Messages,
            query: query.into(),
            generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
            vector: vector.clone(),
            provider: Some(Provider::Cursor),
            days: None,
            exclude_self: false,
            filter: Some(crate::search_filter::Filter::All(vec![
                crate::search_filter::Filter::Project(vec!["/work/cbioportal".into()]),
                crate::search_filter::Filter::Repository(vec!["/work/cbioportal".into()]),
            ])),
        };
        let result: Candidates = service
            .request("search", FIRST)
            .json(&input)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(result.lexical.len(), lexical);
        assert_eq!(result.semantic.len(), 4);
        assert!(
            result
                .lexical
                .iter()
                .chain(&result.semantic)
                .all(|m| m.id != ids[2] && m.id != ids[3])
        );
        if lexical == 2 {
            assert_ne!(result.lexical[0].id, result.lexical[1].id);
        }
    }
    for (id, expected) in [
        (&ids[1], "private-ide-reasoning"),
        (&ids[2], "private-ide-simulated-context"),
        (&ids[3], "Tool 51"),
    ] {
        let input = View {
            node_id: "first".into(),
            id: id.clone(),
            context: 0,
            content: crate::storage::MessageContent::Canonical,
        };
        let messages: Vec<crate::storage::Message> = service
            .request("view", FIRST)
            .json(&input)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].content.contains(expected));
    }
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_native_cursor_ide_publication_preserves_provenance_and_metadata() {
    contract(Service::qdrant(384).await).await;
}
