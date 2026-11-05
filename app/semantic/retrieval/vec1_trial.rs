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
    Streaming,
    #[serde(rename = "metadata_streaming")]
    MetadataStreaming,
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
    phases: Option<crate::storage::vec1::Timings>,
    dense_candidates: Vec<Identity>,
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

fn retrieve(
    storage: &Storage,
    generation: &str,
    vector: &QuantizedVector,
    scope: &Scope,
    limit: usize,
    engine: Engine,
) -> (Vec<SearchHit>, usize, Option<crate::storage::vec1::Timings>) {
    match engine {
        Engine::Exact => {
            let (dense, eligible) = dense_candidates_exact(
                storage,
                generation,
                vector,
                (None, None, Some(scope)),
                limit,
            )
            .unwrap();
            (dense, eligible, None)
        }
        engine => {
            let (candidates, eligible, phases) = match engine {
                Engine::Vec1 => storage.vec1_candidates_profiled(
                    generation,
                    &vector.values,
                    None,
                    None,
                    Some(scope),
                    limit,
                ),
                Engine::Streaming => {
                    storage.vec1_streaming_control(generation, &vector.values, scope, limit)
                }
                Engine::MetadataStreaming => storage.vec1_metadata_streaming_control(
                    generation,
                    &vector.values,
                    scope,
                    limit,
                ),
                Engine::Exact => unreachable!(),
            }
            .unwrap();
            (
                rank_vec1(storage, vector, candidates, limit).unwrap(),
                eligible,
                Some(phases),
            )
        }
    }
}

fn measure(
    storage: &Storage,
    backend: &mut Backend,
    input: &Input,
    trial: &RegisteredCase,
    eligible_ids: &std::collections::BTreeSet<String>,
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
    let (dense, eligible, phases) = retrieve(
        storage,
        &input.generation,
        &vector,
        &scope,
        limit,
        trial.backend,
    );
    let dense_ms = dense_started.elapsed().as_secs_f64() * 1000.0;
    let dense_candidates = dense
        .iter()
        .map(|hit| Identity {
            source: hit.source.clone(),
            id: hit.id.clone(),
        })
        .collect();
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
    let sample = Sample {
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
        phases,
        dense_candidates,
    };
    assert_eq!(
        sample.eligible,
        eligible_ids.len(),
        "canonical eligibility count"
    );
    for candidate in &sample.dense_candidates {
        assert!(
            eligible_ids.contains(&candidate.id),
            "dense candidate violates canonical scope"
        );
    }
    sample
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
    assert_eq!(storage.vec1_projection_mismatches().unwrap(), 0);
    let eligible_ids = input
        .scopes
        .iter()
        .map(|scope| {
            let selection = Scope {
                filter: scope.filter.clone(),
                node: "local".into(),
                match_mode: crate::views::matching::Mode::default(),
            };
            (
                scope.name.clone(),
                storage
                    .vec1_canonical_eligible_ids(&input.generation, &selection)
                    .unwrap(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut backend = crate::semantic::Models::new(path("MASH_TEST_MODELS_DIR"))
        .load()
        .unwrap()
        .expect("installed models");
    // Warm both paths before registered measurements; query embeddings are
    // frozen inputs, so these timings exclude embedding and process startup.
    for engine in [
        Engine::Exact,
        Engine::Streaming,
        Engine::MetadataStreaming,
        Engine::Vec1,
    ] {
        let warm = RegisteredCase {
            trial_number: usize::MAX,
            backend: engine,
            query_index: 0,
            scope: "all".into(),
            unit: SearchUnit::Messages,
            round: 0,
        };
        measure(&storage, &mut backend, &input, &warm, &eligible_ids["all"]);
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path("MASH_VEC1_TRIAL_REPORT"))
        .unwrap();
    let mut output = std::io::BufWriter::new(file);
    for trial in trials {
        let sample = measure(
            &storage,
            &mut backend,
            &input,
            &trial,
            &eligible_ids[&trial.scope],
        );
        println!(
            "trial={} backend={} dense_ms={:.2} pipeline_ms={:.2}",
            sample.trial_number,
            match sample.backend {
                Engine::Exact => "exact",
                Engine::Streaming => "streaming",
                Engine::MetadataStreaming => "metadata_streaming",
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
