//! Public, synthetic relevance judgments exercised through the typed core.
mod cleanup;
use crate::core::{IndexRequest, Operation, Paths, Request, Response};
use crate::search_results::{Results, SearchUnit};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    version: u32,
    documents: Vec<Document>,
    queries: Vec<Query>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    id: String,
    messages: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    text: String,
    relevant: Vec<String>,
    #[serde(default)]
    forbidden: Vec<String>,
    #[serde(default)]
    filters: crate::search_filter::Selection,
}
#[derive(Serialize)]
struct Record<'a> {
    #[serde(rename = "type")]
    kind: crate::role::Role,
    #[serde(rename = "sessionId")]
    session_id: &'a str,
    uuid: String,
    message: Message<'a>,
}
#[derive(Serialize)]
struct Message<'a> {
    role: crate::role::Role,
    content: &'a str,
}

fn corpus() -> Corpus {
    let corpus: Corpus =
        toml::from_str(include_str!("../../fixtures/search-relevance.toml")).unwrap();
    assert_eq!(corpus.version, 1);
    corpus
}

pub(crate) fn fixture() -> (tempfile::TempDir, Paths) {
    let corpus = corpus();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("claude");
    std::fs::create_dir(&source).unwrap();
    for document in &corpus.documents {
        let mut file =
            std::fs::File::create(source.join(format!("{}.jsonl", document.id))).unwrap();
        for (i, content) in document.messages.iter().enumerate() {
            let record = Record {
                kind: crate::role::Role::User,
                session_id: &document.id,
                uuid: format!("{}-{i}", document.id),
                message: Message {
                    role: crate::role::Role::User,
                    content,
                },
            };
            serde_json::to_writer(&mut file, &record).unwrap();
            writeln!(file).unwrap();
        }
    }
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version = 1
            providers = {claude-code = {roots = [(source.to_str().unwrap())]}}
            index = {all_history = true}
        })
        .unwrap(),
    )
    .unwrap();
    let paths = Paths {
        config: Some(config),
        db: Some(root.path().join("index.sqlite3")),
        models_dir: Some(
            std::env::var_os("MASH_TEST_MODELS_DIR")
                .expect("explicit model directory")
                .into(),
        ),
    };
    (root, paths)
}

fn regressions(query: &Query, ids: &[&str], unit: SearchUnit) -> Vec<String> {
    let mut failures = Vec::new();
    if query.relevant.is_empty() {
        if !ids.is_empty() {
            failures.push(format!(
                "{unit:?} {:?}: should abstain, got {ids:?}",
                query.text
            ));
        }
    } else if ids
        .first()
        .is_none_or(|id| !query.relevant.iter().any(|expected| expected == id))
    {
        failures.push(format!(
            "{unit:?} {:?}: expected relevant first result {:?}, got {ids:?}",
            query.text, query.relevant
        ));
    }
    if ids
        .iter()
        .any(|id| query.forbidden.iter().any(|forbidden| forbidden == id))
    {
        failures.push(format!(
            "{unit:?} {:?}: forbidden results {ids:?}",
            query.text
        ));
    }
    failures
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn labeled_synthetic_relevance_matrix_uses_real_models_and_the_application_core() {
    let (_root, paths) = fixture();
    crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .unwrap();
    assert_eq!(
        evaluate(&paths, false).await,
        Vec::<String>::new(),
        "relevance regressions"
    );
}

pub(crate) async fn evaluate(paths: &Paths, shared: bool) -> Vec<String> {
    let corpus = corpus();
    let mut failures = Vec::new();
    for unit in [SearchUnit::Messages, SearchUnit::Chats] {
        let mut precision = 0.0;
        let mut recall = 0.0;
        let mut reciprocal_rank = 0.0;
        let mut positive_count = 0.0;
        for query in &corpus.queries {
            let started = std::time::Instant::now();
            let Response::Search(response) = crate::core::execute(Request {
                paths: paths.clone(),
                operation: Operation::Search {
                    shared_vector_search: None,
                    fusion: crate::views::config::FusionOverrides::default(),
                    ranking: None,
                    diagnostics: false,
                    query: query.text.clone(),
                    intent: None,
                    limit: Some(5),
                    offset: 0,
                    unit: Some(unit),
                    filters: Box::new(query.filters.clone()),
                    shared,
                    local: !shared,
                },
            })
            .await
            .unwrap() else {
                panic!("search response");
            };
            assert_eq!(
                response.shared, shared,
                "shared search must not fall back to local: {:?}",
                response.shared_error
            );
            assert!(response.shared_error.is_none());
            let ids = match &response.results {
                Results::Messages(hits) => hits
                    .iter()
                    .map(|hit| hit.conversation_id.as_str())
                    .collect::<Vec<_>>(),
                Results::Chats(hits) => hits
                    .iter()
                    .map(|hit| hit.conversation_id.as_str())
                    .collect::<Vec<_>>(),
            };
            eprintln!(
                "relevance shared={shared} {unit:?} {:?}: {ids:?} elapsed_ms={}",
                query.text,
                started.elapsed().as_millis()
            );
            let relevant = |id: &&str| query.relevant.iter().any(|expected| expected == *id);
            if !query.relevant.is_empty() {
                positive_count += 1.0;
                let matching = ids.iter().filter(|id| relevant(id)).count();
                // This fixture is deliberately small; counts fit exactly in u32/f64.
                precision += f64::from(u32::try_from(matching).unwrap())
                    / f64::from(u32::try_from(ids.len().max(1)).unwrap());
                recall += f64::from(
                    u32::try_from(
                        query
                            .relevant
                            .iter()
                            .filter(|id| ids.contains(&id.as_str()))
                            .count(),
                    )
                    .unwrap(),
                ) / f64::from(u32::try_from(query.relevant.len()).unwrap());
                reciprocal_rank += ids.iter().position(relevant).map_or(0.0, |rank| {
                    1.0 / f64::from(u32::try_from(rank + 1).unwrap())
                });
            }
            failures.extend(regressions(query, &ids, unit));
        }
        eprintln!(
            "relevance shared={shared} {unit:?}: precision@returned5={:.3} recall@5={:.3} MRR@5={:.3}",
            precision / positive_count,
            recall / positive_count,
            reciprocal_rank / positive_count
        );
    }
    failures
}
