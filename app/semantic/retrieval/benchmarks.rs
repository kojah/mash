//! Opt-in real-corpus comparison data for local exact and ANN retrieval.
//! Reports contain private vector identities and must remain outside Git.
use super::*;
use crate::search_filter::{Filter, Scope};
use crate::semantic::Models;
use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

mod pipeline;

const QUERIES: [&str; 7] = [
    "deploy plan",
    "SQLite indexing performance",
    "keep embedding model warm between CLI searches",
    "message ordering and parent references",
    "compression of canonical chat history",
    "authentication token refresh",
    "search without SSH",
];

#[derive(Serialize)]
struct NativeIdentity {
    rowid: i64,
    message_id: String,
}

#[derive(Serialize)]
struct ScopeReport {
    name: &'static str,
    filter: Filter,
    native_rowids: Vec<i64>,
    connector_ids: Vec<i64>,
    native_load_ms: f64,
    connector_load_ms: f64,
}

#[derive(Serialize)]
struct Ranked {
    connector: bool,
    id: i64,
    score: f32,
}

#[derive(Serialize)]
struct Measurement {
    scope: &'static str,
    repetition: usize,
    cached_score_ms: f64,
    dense_with_hydration_ms: f64,
    eligible: usize,
    expected: Vec<Ranked>,
}

#[derive(Serialize)]
struct QueryReport {
    query: &'static str,
    vector: Vec<i8>,
    norm: f32,
    embed_ms: f64,
    measurements: Vec<Measurement>,
}

#[derive(Serialize)]
struct Report {
    generation: &'static str,
    optimized: bool,
    cutoff: i64,
    native_identities: Vec<NativeIdentity>,
    scopes: Vec<ScopeReport>,
    queries: Vec<QueryReport>,
}

fn merged_scores(
    query: &QuantizedVector,
    native: &SemanticChunks,
    connector: &SemanticChunks,
) -> Vec<Ranked> {
    let mut ranked = rank_quantized(query, native, 20)
        .into_iter()
        .map(|(id, score)| Ranked {
            connector: false,
            id,
            score,
        })
        .chain(
            rank_quantized(query, connector, 20)
                .into_iter()
                .map(|(id, score)| Ranked {
                    connector: true,
                    id,
                    score,
                }),
        )
        .collect::<Vec<_>>();
    ranked.sort_unstable_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then((a.connector, a.id).cmp(&(b.connector, b.id)))
    });
    ranked.truncate(20);
    ranked
}

fn scopes(cutoff: i64) -> [(&'static str, Scope); 7] {
    let slack = Filter::Kind(vec!["slack:message".into()]);
    let claude = Filter::Kind(vec!["claude-code:message".into()]);
    [
        ("all", Filter::All(vec![])),
        ("slack", slack.clone()),
        ("claude", claude.clone()),
        (
            "slack-recent",
            Filter::All(vec![slack.clone(), Filter::After(cutoff)]),
        ),
        ("slack-or-claude", Filter::Any(vec![slack.clone(), claude])),
        ("not-slack", Filter::Not(Box::new(slack))),
        (
            "empty",
            Filter::Chat(vec!["mash-ann-evaluation-no-such-chat".into()]),
        ),
    ]
    .map(|(name, filter)| {
        (
            name,
            Scope {
                match_mode: crate::views::matching::Mode::Strict,
                filter,
                node: "local".into(),
            },
        )
    })
}

type LoadedVectors = Vec<(SemanticChunks, SemanticChunks)>;

fn load_scopes(
    storage: &Storage,
    scopes: &[(&'static str, Scope)],
) -> (Vec<ScopeReport>, LoadedVectors) {
    let mut reports = Vec::new();
    let mut loaded = Vec::new();
    for (name, scope) in scopes {
        let start = Instant::now();
        let native = storage
            .semantic_chunks_filtered(embedding_generation(), None, None, Some(scope))
            .unwrap();
        let native_load_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let vectors = storage
            .connector_vectors(embedding_generation(), None, Some(scope))
            .unwrap();
        let dimensions = vectors
            .first()
            .map_or(native.dimensions, |(_, _, v)| v.len());
        let connector = connector_chunks(vectors, dimensions);
        let connector_load_ms = start.elapsed().as_secs_f64() * 1000.0;
        reports.push(ScopeReport {
            name,
            filter: scope.filter.clone(),
            native_rowids: native
                .chunks
                .iter()
                .flat_map(|chunk| {
                    chunk
                        .message_rowids
                        .iter()
                        .zip(&chunk.eligible)
                        .filter_map(|(id, eligible)| eligible.then_some(*id))
                })
                .collect(),
            connector_ids: connector
                .chunks
                .iter()
                .flat_map(|chunk| chunk.message_rowids.iter().copied())
                .collect(),
            native_load_ms,
            connector_load_ms,
        });
        loaded.push((native, connector));
    }
    (reports, loaded)
}

#[test]
#[ignore = "manual corpus benchmark requires installed models, MASH_TEST_SEARCH_DB and MASH_TEST_VECTOR_REPORT"]
fn benchmark_local_filtered_vectors() {
    let models = PathBuf::from(std::env::var_os("MASH_TEST_MODELS_DIR").expect("installed models"));
    let database = PathBuf::from(std::env::var_os("MASH_TEST_SEARCH_DB").expect("frozen corpus"));
    let output =
        PathBuf::from(std::env::var_os("MASH_TEST_VECTOR_REPORT").expect("private report path"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    let storage = Storage::open_search(&database, embedding_generation()).unwrap();
    let mut backend = Models::new(models)
        .load()
        .unwrap()
        .expect("installed models");
    let cutoff = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
        - 30 * 86400;
    let scopes = scopes(cutoff);
    let (scope_reports, loaded) = load_scopes(&storage, &scopes);
    let mut report = Report {
        generation: embedding_generation(),
        optimized: !cfg!(debug_assertions),
        cutoff,
        native_identities: Vec::new(),
        scopes: scope_reports,
        queries: Vec::new(),
    };
    // Map stable native vector addresses to the message IDs in message_embeddings.
    // This happens outside measured retrieval and does not export history text.
    for rowid in &report.scopes[0].native_rowids {
        let hit = storage.search_hits(&[*rowid]).unwrap().pop().unwrap();
        report.native_identities.push(NativeIdentity {
            rowid: *rowid,
            message_id: hit.id,
        });
    }
    for query in QUERIES {
        let start = Instant::now();
        let vector = quantize_vector(&backend.embed(&[query]).unwrap().pop().unwrap());
        let embed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut measurements = Vec::new();
        for ((name, scope), (native, connector)) in scopes.iter().zip(&loaded) {
            for repetition in 0..3 {
                let start = Instant::now();
                let expected = merged_scores(&vector, native, connector);
                let cached_score_ms = start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                let (hits, eligible) = dense_candidates(
                    &storage,
                    embedding_generation(),
                    &vector,
                    (None, None, Some(scope)),
                    20,
                )
                .unwrap();
                let dense_with_hydration_ms = start.elapsed().as_secs_f64() * 1000.0;
                assert_eq!(
                    eligible,
                    eligible_vectors(native) + eligible_vectors(connector)
                );
                assert_eq!(hits.len(), expected.len());
                for (hit, ranked) in hits.iter().zip(&expected) {
                    assert_eq!(hit.semantic_score, Some(ranked.score));
                }
                println!(
                    "{}",
                    serde_json::json!({"query":query,"scope":name,"repetition":repetition,"eligible":eligible,"cached_score_ms":cached_score_ms,"dense_with_hydration_ms":dense_with_hydration_ms})
                );
                measurements.push(Measurement {
                    scope: name,
                    repetition,
                    cached_score_ms,
                    dense_with_hydration_ms,
                    eligible,
                    expected,
                });
            }
        }
        report.queries.push(QueryReport {
            query,
            vector: vector.values.into_iter().map(u8::cast_signed).collect(),
            norm: vector.norm,
            embed_ms,
            measurements,
        });
    }
    serde_json::to_writer(&mut file, &report).unwrap();
    file.write_all(b"\n").unwrap();
}
