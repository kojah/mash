//! Manual real-corpus diagnostics; no downloads, mutation, or query-result cache.
use super::*;
use crate::search_results::{Results, SearchUnit};
use crate::storage::Storage;
use std::time::Instant;

mod embedding;

const QUERIES: [&str; 7] = [
    "cbioportal",
    "cornucopia",
    "initial indexing progress ETA",
    "voyage-code-4",
    "postgres redis performance",
    "show me discussions about keeping a model warm between CLI searches",
    "search without SSH",
];

#[derive(Debug, PartialEq, Eq)]
enum Keys {
    Messages(Vec<String>),
    Chats(Vec<(String, Vec<String>)>),
}

fn keys(results: Results) -> Keys {
    match results {
        Results::Messages(hits) => Keys::Messages(hits.into_iter().map(|hit| hit.id).collect()),
        Results::Chats(hits) => Keys::Chats(
            hits.into_iter()
                .map(|hit| {
                    (
                        hit.conversation_id,
                        hit.matches.into_iter().map(|hit| hit.message_id).collect(),
                    )
                })
                .collect(),
        ),
    }
}

fn fixture() -> (
    generations::SelectedAssets,
    Storage,
    crate::search_filter::Scope,
) {
    let models = PathBuf::from(std::env::var_os("MASH_TEST_MODELS_DIR").expect("installed models"));
    let selected = generations::select(&models, false).unwrap();
    let path = PathBuf::from(
        std::env::var_os("MASH_TEST_SEARCH_DB").expect("explicit refreshed test index"),
    );
    let storage = Storage::open_search(&path, crate::semantic::embedding_generation()).unwrap();
    let filters = crate::search_filter::Selection {
        role: vec![crate::role::Role::User],
        days: Some(14),
        ..Default::default()
    }
    .resolve("local")
    .unwrap();
    (selected, storage, filters)
}

fn backend(root: &Path, threads: Option<usize>) -> Backend {
    Backend {
        embedding: Some(load_local_embedding(root, threads).unwrap()),
        remote: None,
        reranker: Some(load_reranker_with_threads(root, threads).unwrap()),
        assets_lease: None,
    }
}

#[derive(Serialize)]
struct Sample<'a> {
    query: &'a str,
    unit: SearchUnit,
    threads: Option<usize>,
    milliseconds: f64,
    same_results: bool,
}

#[test]
#[ignore = "manual benchmark requires MASH_TEST_SEARCH_DB and installed MASH_TEST_MODELS_DIR"]
fn benchmark_real_search_thread_budget() {
    let (selected, storage, filters) = fixture();
    let mut expected = Vec::new();
    for threads in [None, Some(1), Some(2), Some(4), Some(8), None, Some(8)] {
        let mut backend = backend(&selected.root, threads);
        // Warm model execution and the exact-vector cache before comparison.
        crate::semantic::hybrid_search_filtered(
            crate::semantic::embedding_generation(),
            &storage,
            &mut backend,
            QUERIES[0],
            crate::gold::config::Options::new(5, SearchUnit::Chats),
            (None, None),
            Some(&filters),
        )
        .unwrap();
        let mut index = 0;
        for unit in [SearchUnit::Chats, SearchUnit::Messages] {
            for query in QUERIES {
                let start = Instant::now();
                let hits = crate::semantic::hybrid_search_filtered(
                    crate::semantic::embedding_generation(),
                    &storage,
                    &mut backend,
                    query,
                    crate::gold::config::Options::new(5, unit),
                    (None, None),
                    Some(&filters),
                )
                .unwrap();
                let actual = keys(crate::search_results::collect(hits, unit, 5, query));
                let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
                let same_results = if expected.len() <= index {
                    expected.push(actual);
                    true
                } else {
                    expected[index] == actual
                };
                assert!(
                    same_results,
                    "ranking changed for {query}, {unit:?}, {threads:?}"
                );
                println!(
                    "{}",
                    serde_json::to_string(&Sample {
                        query,
                        unit,
                        threads,
                        milliseconds,
                        same_results
                    })
                    .unwrap()
                );
                index += 1;
            }
        }
    }
}

#[derive(Serialize)]
struct BatchSample<'a> {
    query: &'a str,
    batch_size: usize,
    documents: usize,
    milliseconds: f64,
    same_order: bool,
    max_score_difference: f32,
}

#[test]
#[ignore = "manual benchmark requires MASH_TEST_SEARCH_DB and installed MASH_TEST_MODELS_DIR"]
fn benchmark_real_search_rerank_batches() {
    let (selected, storage, filters) = fixture();
    let mut backend = backend(&selected.root, None);
    for query in QUERIES {
        let vector = crate::semantic::shared_query_vector(&mut backend, query).unwrap();
        let (hits, _) = crate::semantic::local_candidates(
            &storage,
            crate::semantic::embedding_generation(),
            query,
            &vector,
            (None, None, SearchUnit::Chats),
            Some(&filters),
            crate::gold::config::Fusion::default(),
        )
        .unwrap();
        let hits = crate::search_results::diversify(hits, SearchUnit::Chats);
        let selection =
            crate::search_ranking::select(&mut backend, query, hits, SearchUnit::Chats, 5).unwrap();
        let documents = selection.hits[..selection.count]
            .iter()
            .map(|hit| {
                let context = storage
                    .view_content(&hit.id, 2, crate::storage::MessageContent::SearchProjection)
                    .unwrap();
                let document = crate::search_results::contextual_document(hit, query, &context);
                crate::search_results::rerank_document(&document, query).into_owned()
            })
            .collect::<Vec<_>>();
        let refs = documents.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        let expected = backend.rerank_batched(query, &refs, 4).unwrap();
        for batch_size in [4, 1, 2, 8, 16, 16, 8, 4] {
            let start = Instant::now();
            let actual = backend.rerank_batched(query, &refs, batch_size).unwrap();
            let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
            let same_order = expected
                .iter()
                .map(|r| r.index)
                .eq(actual.iter().map(|r| r.index));
            let max_score_difference = expected
                .iter()
                .map(|e| {
                    (e.score - actual.iter().find(|a| a.index == e.index).unwrap().score).abs()
                })
                .fold(0.0, f32::max);
            assert!(
                same_order,
                "reranking order changed for {query}, batch {batch_size}"
            );
            assert!(
                max_score_difference <= 1e-5,
                "reranking scores changed for {query}, batch {batch_size}: {max_score_difference}"
            );
            println!(
                "{}",
                serde_json::to_string(&BatchSample {
                    query,
                    batch_size,
                    documents: refs.len(),
                    milliseconds,
                    same_order,
                    max_score_difference
                })
                .unwrap()
            );
        }
    }
}
