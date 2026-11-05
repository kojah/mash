//! Opt-in paired comparison through real hydration, fusion and reranking.
use super::*;
use crate::search_filter::{Filter, Scope};
use crate::search_results::{Page, Results, SearchUnit};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Deserialize)]
struct Input {
    generation: String,
    queries: Vec<QueryInput>,
    scopes: Vec<NamedScope>,
}
#[derive(Deserialize)]
struct QueryInput {
    query: String,
    vector: Vec<i8>,
    norm: f32,
}
#[derive(Deserialize)]
struct NamedScope {
    name: String,
    filter: Filter,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Engine {
    Exact,
    Vec1,
}
#[derive(Deserialize)]
struct RegisteredCase {
    trial_number: usize,
    backend: Engine,
    query_index: usize,
    scope: String,
    unit: SearchUnit,
    round: usize,
}
#[derive(Serialize)]
struct Identity {
    source: crate::source::Source,
    id: String,
}
#[derive(Serialize)]
struct Snapshot {
    identities: Vec<Identity>,
    evidence: Vec<Vec<String>>,
    scores: Vec<f32>,
}
#[derive(Serialize)]
struct Sample {
    trial_number: usize,
    backend: Engine,
    query_index: usize,
    scope: String,
    unit: SearchUnit,
    round: usize,
    eligible: usize,
    dense_ms: f64,
    pipeline_ms: f64,
    result: Snapshot,
}

fn snapshot(hits: Vec<SearchHit>, unit: SearchUnit, query: &str) -> Snapshot {
    match crate::search_results::collect(
        hits,
        unit,
        Page::first(20),
        query,
        crate::redaction::Policy::ON,
    )
    .results
    {
        Results::Messages(hits) => Snapshot {
            identities: hits
                .iter()
                .map(|h| Identity {
                    source: h.source.clone(),
                    id: h.id.clone(),
                })
                .collect(),
            evidence: Vec::new(),
            scores: hits
                .iter()
                .map(|h| h.rerank_score.unwrap_or(h.fusion_score))
                .collect(),
        },
        Results::Chats(hits) => Snapshot {
            identities: hits
                .iter()
                .map(|h| Identity {
                    source: h.source.clone(),
                    id: h.conversation_id.clone(),
                })
                .collect(),
            evidence: hits
                .iter()
                .map(|h| h.matches.iter().map(|m| m.message_id.clone()).collect())
                .collect(),
            scores: hits.iter().map(|h| h.score).collect(),
        },
    }
}

fn measure(
    storage: &Storage,
    backend: &mut Backend,
    input: &Input,
    trial: &RegisteredCase,
) -> Sample {
    let query = &input.queries[trial.query_index];
    let filter = &input
        .scopes
        .iter()
        .find(|s| s.name == trial.scope)
        .expect("registered scope")
        .filter;
    let scope = Scope {
        filter: filter.clone(),
        node: "local".into(),
        match_mode: crate::views::matching::Mode::default(),
    };
    let vector = QuantizedVector {
        values: query.vector.iter().map(|v| v.to_ne_bytes()[0]).collect(),
        norm: query.norm,
    };
    let started = Instant::now();
    let limit = trial.unit.candidate_limit();
    let lexical = storage
        .search_filtered(&query.query, limit, None, None, Some(&scope))
        .unwrap();
    let dense_started = Instant::now();
    let (dense, eligible) = match trial.backend {
        Engine::Exact => dense_candidates_exact(
            storage,
            &input.generation,
            &vector,
            (None, None, Some(&scope)),
            limit,
        ),
        Engine::Vec1 => dense_candidates(
            storage,
            &input.generation,
            &vector,
            (None, None, Some(&scope)),
            limit,
        ),
    }
    .unwrap();
    let dense_ms = dense_started.elapsed().as_secs_f64() * 1000.0;
    let hits = finish_hybrid(
        storage,
        backend,
        &query.query,
        Options::new(trial.unit),
        lexical,
        dense,
    )
    .unwrap();
    let result = snapshot(hits, trial.unit, &query.query);
    Sample {
        trial_number: trial.trial_number,
        backend: trial.backend,
        query_index: trial.query_index,
        scope: trial.scope.clone(),
        unit: trial.unit,
        round: trial.round,
        eligible,
        dense_ms,
        pipeline_ms: started.elapsed().as_secs_f64() * 1000.0,
        result,
    }
}

#[test]
#[ignore = "explicit private Vec1 corpus, registered trial plan, query vectors and installed models"]
fn vec1_local_gold_trial() {
    let path = |name| PathBuf::from(std::env::var_os(name).expect("explicit trial input"));
    let input: Input =
        serde_json::from_slice(&std::fs::read(path("MASH_VEC1_QUERY_INPUT")).unwrap()).unwrap();
    let trials: Vec<RegisteredCase> =
        serde_json::from_slice(&std::fs::read(path("MASH_VEC1_TRIAL_PLAN")).unwrap()).unwrap();
    let database = path("MASH_VEC1_TRIAL_DB");
    let storage = Storage::open_search(&database, &input.generation).unwrap();
    let mut backend = crate::semantic::Models::new(path("MASH_TEST_MODELS_DIR"))
        .load()
        .unwrap()
        .expect("installed models");
    // Warm both paths before registered measurements; query embeddings are
    // frozen inputs, so these timings exclude embedding and process startup.
    for engine in [Engine::Exact, Engine::Vec1] {
        let warm = RegisteredCase {
            trial_number: usize::MAX,
            backend: engine,
            query_index: 0,
            scope: "all".into(),
            unit: SearchUnit::Messages,
            round: 0,
        };
        measure(&storage, &mut backend, &input, &warm);
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path("MASH_VEC1_TRIAL_REPORT"))
        .unwrap();
    let mut output = std::io::BufWriter::new(file);
    for trial in trials {
        let sample = measure(&storage, &mut backend, &input, &trial);
        println!(
            "trial={} backend={} dense_ms={:.2} pipeline_ms={:.2}",
            sample.trial_number,
            match sample.backend {
                Engine::Exact => "exact",
                Engine::Vec1 => "vec1",
            },
            sample.dense_ms,
            sample.pipeline_ms
        );
        serde_json::to_writer(&mut output, &sample).unwrap();
        output.write_all(b"\n").unwrap();
        output.flush().unwrap();
    }
}
