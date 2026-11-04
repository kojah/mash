//! Warm-process pipeline replay using privately exported ANN candidate lists.
use super::*;
use crate::search_results::{Page, Results, SearchUnit};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Candidate {
    connector: bool,
    id: i64,
    score: f32,
}

#[derive(Deserialize)]
struct Case {
    backend: String,
    query: String,
    scope: String,
    filter: Filter,
    vector: Vec<i8>,
    norm: f32,
    unit: SearchUnit,
    candidate_limit: usize,
    eligible: usize,
    candidates: Vec<Candidate>,
    retrieval_ms: f64,
    rescore_ms: f64,
    scope_prepare_ms: f64,
}

#[derive(Deserialize)]
struct Input {
    generation: String,
    cases: Vec<Case>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
struct Identity {
    source: crate::source::Source,
    id: String,
}

#[derive(Clone, Serialize)]
struct Snapshot {
    identities: Vec<Identity>,
    evidence: Vec<Vec<String>>,
    scores: Vec<f32>,
}

#[derive(Serialize)]
struct Sample<'a> {
    backend: &'a str,
    query: &'a str,
    scope: &'a str,
    unit: SearchUnit,
    round: usize,
    exact_ms: f64,
    replay_ms: f64,
    sql_prefilter_ms: f64,
    retrieval_ms: f64,
    rescore_ms: f64,
    scope_prepare_ms: f64,
    estimated_pipeline_ms: f64,
    shared_results: usize,
    same_order: bool,
    same_evidence: bool,
    exact: Snapshot,
    candidate: Snapshot,
}

fn collect(hits: Vec<SearchHit>, case: &Case) -> Snapshot {
    match crate::search_results::collect(
        hits,
        case.unit,
        Page::first(20),
        &case.query,
        crate::redaction::Policy::ON,
    )
    .results
    {
        Results::Messages(hits) => Snapshot {
            identities: hits
                .iter()
                .map(|hit| Identity {
                    source: hit.source.clone(),
                    id: hit.id.clone(),
                })
                .collect(),
            evidence: Vec::new(),
            scores: hits
                .iter()
                .map(|hit| hit.rerank_score.unwrap_or(hit.fusion_score))
                .collect(),
        },
        Results::Chats(hits) => Snapshot {
            identities: hits
                .iter()
                .map(|hit| Identity {
                    source: hit.source.clone(),
                    id: hit.conversation_id.clone(),
                })
                .collect(),
            evidence: hits
                .iter()
                .map(|hit| hit.matches.iter().map(|m| m.message_id.clone()).collect())
                .collect(),
            scores: hits.iter().map(|hit| hit.score).collect(),
        },
    }
}

fn scope(case: &Case) -> Scope {
    Scope {
        match_mode: crate::views::matching::Mode::default(),
        filter: case.filter.clone(),
        node: "local".into(),
    }
}

fn prefilter(storage: &Storage, views: &rusqlite::Connection, case: &Case, scope: &Scope) -> f64 {
    if case.backend != "usearch-adaptive"
        || matches!(&scope.filter, Filter::All(children) if children.is_empty())
    {
        return 0.0;
    }
    let start = Instant::now();
    let native = storage.filtered_semantic_rowids(scope).unwrap();
    let predicate = scope.filter.connector_sql(Some(&scope.node));
    let mut statement = views
        .prepare(&format!(
            "SELECT u.id FROM connector_embeddings e JOIN connector_units u ON u.id=e.unit_id
         WHERE e.generation=?1 AND ({predicate})"
        ))
        .unwrap();
    let connector = statement
        .query_map([embedding_generation()], |row| row.get::<_, i64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(native.len() + connector.len(), case.eligible);
    assert!(case.candidates.iter().all(|c| if c.connector {
        connector.contains(&c.id)
    } else {
        native.contains(&c.id)
    }));
    milliseconds
}

fn replay(storage: &Storage, backend: &mut Backend, case: &Case, scope: &Scope) -> Snapshot {
    let limit = case.unit.candidate_limit();
    assert_eq!(case.candidate_limit, limit);
    assert!(case.candidates.len() <= limit);
    let lexical = storage
        .search_filtered(&case.query, limit, None, None, Some(scope))
        .unwrap();
    let vector = quantize_vector(
        &backend
            .embed(&[case.query.as_str()])
            .unwrap()
            .pop()
            .unwrap(),
    );
    assert_eq!(
        vector
            .values
            .iter()
            .copied()
            .map(u8::cast_signed)
            .collect::<Vec<_>>(),
        case.vector
    );
    assert_eq!(vector.norm, case.norm);
    assert!(
        case.candidates
            .iter()
            .all(|c| c.score.is_finite() && (-1.0..=1.0).contains(&c.score))
    );
    let ranked = case
        .candidates
        .iter()
        .map(|c| (c.connector, c.id, c.score))
        .collect();
    let semantic = hydrate_ranked(storage, ranked).unwrap();
    let hits = finish_hybrid(
        storage,
        backend,
        &case.query,
        Options::new(case.unit),
        lexical,
        semantic,
    )
    .unwrap();
    collect(hits, case)
}

fn load_inputs() -> Vec<Case> {
    let paths = std::env::var_os("MASH_TEST_ANN_INPUTS").expect("private ANN candidate files");
    let mut cases = Vec::new();
    for path in std::env::split_paths(&paths) {
        let input: Input = serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
        assert_eq!(input.generation, embedding_generation());
        assert!(
            input
                .cases
                .iter()
                .all(|c| matches!(c.backend.as_str(), "usearch-adaptive" | "edge-metadata"))
        );
        cases.extend(input.cases);
    }
    assert!(!cases.is_empty());
    cases
}

fn measure<'a>(
    storage: &Storage,
    views: &rusqlite::Connection,
    backend: &mut Backend,
    case: &'a Case,
    round: usize,
    exact: &(Snapshot, f64),
) -> Sample<'a> {
    let scope = scope(case);
    let sql_prefilter_ms = prefilter(storage, views, case, &scope);
    let start = Instant::now();
    let candidate = replay(storage, backend, case, &scope);
    let replay_ms = start.elapsed().as_secs_f64() * 1000.0;
    // The narrow USearch adapter uses exhaustive canonical i8 scoring, so its
    // injected candidates must preserve the complete pipeline's exact output.
    if case.backend == "usearch-adaptive" && case.eligible <= 4096 {
        assert!(candidate.identities == exact.0.identities);
        assert_eq!(candidate.evidence, exact.0.evidence);
    }
    Sample {
        backend: &case.backend,
        query: &case.query,
        scope: &case.scope,
        unit: case.unit,
        round,
        exact_ms: exact.1,
        replay_ms,
        sql_prefilter_ms,
        retrieval_ms: case.retrieval_ms,
        rescore_ms: case.rescore_ms,
        scope_prepare_ms: case.scope_prepare_ms,
        estimated_pipeline_ms: replay_ms
            + sql_prefilter_ms
            + case.retrieval_ms
            + case.rescore_ms
            + case.scope_prepare_ms,
        shared_results: candidate
            .identities
            .iter()
            .filter(|id| exact.0.identities.contains(id))
            .count(),
        same_order: candidate.identities == exact.0.identities,
        same_evidence: candidate.evidence == exact.0.evidence,
        exact: exact.0.clone(),
        candidate,
    }
}

#[test]
#[ignore = "manual pipeline benchmark requires frozen corpus, installed models and private ANN candidates"]
fn benchmark_local_ann_pipeline() {
    let cases = load_inputs();
    let database = PathBuf::from(std::env::var_os("MASH_TEST_SEARCH_DB").expect("frozen corpus"));
    let models = PathBuf::from(std::env::var_os("MASH_TEST_MODELS_DIR").expect("installed models"));
    let output = PathBuf::from(
        std::env::var_os("MASH_TEST_PIPELINE_REPORT").expect("private pipeline report"),
    );
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    let storage = Storage::open_search(&database, embedding_generation()).unwrap();
    let views = rusqlite::Connection::open_with_flags(
        crate::storage::schema::views_path(&database),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut backend = Models::new(models)
        .load()
        .unwrap()
        .expect("installed models");
    let mut samples = Vec::new();
    // Warm both model execution and contextual retrieval outside the measured rounds.
    hybrid_search_filtered(
        embedding_generation(),
        &storage,
        &mut backend,
        &cases[0].query,
        Options::new(cases[0].unit),
        (None, None),
        Some(&scope(&cases[0])),
    )
    .unwrap();
    for round in 0..2 {
        let mut exact = HashMap::new();
        let order = if round == 0 {
            (0..cases.len()).collect::<Vec<_>>()
        } else {
            (0..cases.len()).rev().collect()
        };
        for index in order {
            let case = &cases[index];
            let key = (
                &case.query,
                serde_json::to_string(&case.filter).unwrap(),
                case.unit == SearchUnit::Chats,
            );
            let reference = exact.entry(key).or_insert_with(|| {
                let start = Instant::now();
                let hits = hybrid_search_filtered(
                    embedding_generation(),
                    &storage,
                    &mut backend,
                    &case.query,
                    Options::new(case.unit),
                    (None, None),
                    Some(&scope(case)),
                )
                .unwrap();
                let result = collect(hits, case);
                (result, start.elapsed().as_secs_f64() * 1000.0)
            });
            let sample = measure(&storage, &views, &mut backend, case, round, reference);
            println!(
                "{}",
                serde_json::json!({"backend":sample.backend,"query":sample.query,"scope":sample.scope,"unit":sample.unit,"round":round,"exact_ms":sample.exact_ms,"estimated_pipeline_ms":sample.estimated_pipeline_ms,"shared_results":sample.shared_results,"exact_results":sample.exact.identities.len(),"same_order":sample.same_order})
            );
            samples.push(sample);
        }
    }
    serde_json::to_writer(&mut file, &samples).unwrap();
    file.write_all(b"\n").unwrap();
}
