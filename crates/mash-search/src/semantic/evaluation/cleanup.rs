//! Explicit, read-only paired-text retrieval ablation; no production preprocessing.
use crate::semantic::{EmbeddingPool, Models};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

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
    raw: String,
    cleaned: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    text: String,
    relevant: Vec<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Inference {
    Singleton,
    DocumentBatch,
}

#[derive(Serialize)]
struct Trial {
    inference: Inference,
    raw_ms: f64,
    cleaned_ms: f64,
    queries: Vec<Comparison>,
    document_cosines: Vec<f64>,
}

#[derive(Serialize)]
struct Comparison {
    query: usize,
    raw_rank: usize,
    cleaned_rank: usize,
    raw_recall_at_five: f64,
    cleaned_recall_at_five: f64,
}

#[derive(Serialize)]
struct Report {
    version: u32,
    corpus_fingerprint: String,
    generation: String,
    documents: usize,
    changed_documents: usize,
    trials: Vec<Trial>,
}

fn cosine(left: &[f64], right: &[f64]) -> f64 {
    assert_eq!(left.len(), right.len());
    let norm = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    let denominator = norm(left) * norm(right);
    assert!(denominator.is_finite() && denominator > 0.0);
    left.iter().zip(right).map(|(a, b)| a * b).sum::<f64>() / denominator
}

fn vectors(pool: &mut EmbeddingPool, texts: &[&str], inference: Inference) -> Vec<Vec<f64>> {
    texts
        .chunks(64)
        .flat_map(|chunk| match inference {
            Inference::Singleton => pool.singleton_label_vectors(chunk).unwrap(),
            Inference::DocumentBatch => pool.shared_vectors(chunk).unwrap(),
        })
        .collect()
}

fn rank(query: &[f64], vectors: &[Vec<f64>]) -> Vec<usize> {
    let mut scores: Vec<_> = vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| (index, cosine(query, vector)))
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scores.into_iter().map(|(index, _)| index).collect()
}

#[test]
#[ignore = "manual paired embedding retrieval requires explicit reviewed corpus, local models and report path"]
fn benchmark_paired_embedding_cleanup() {
    let input = PathBuf::from(std::env::var_os("MASH_TEST_CLEANUP_CORPUS").unwrap());
    let bytes = std::fs::read(input).unwrap();
    assert!(bytes.len() <= 1_048_576);
    let corpus: Corpus = toml::from_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(corpus.version, 1);
    assert!((1..=256).contains(&corpus.documents.len()));
    assert!((1..=128).contains(&corpus.queries.len()));
    let ids: BTreeSet<_> = corpus.documents.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids.len(), corpus.documents.len());
    for document in &corpus.documents {
        assert!(!document.id.is_empty() && !document.cleaned.trim().is_empty());
        assert!(document.raw.len() <= 8192);
        // The first experiment removes only whole leading/trailing clauses.
        // Internal token erasure/rewriting needs a separately reviewed experiment.
        assert!(document.raw.contains(&document.cleaned));
    }
    for query in &corpus.queries {
        assert!(!query.text.trim().is_empty() && query.text.len() <= 2048);
        let relevant: BTreeSet<_> = query.relevant.iter().map(String::as_str).collect();
        assert!(!relevant.is_empty() && relevant.len() == query.relevant.len());
        assert!(relevant.is_subset(&ids));
    }
    let models = Models::new(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into());
    models.require_indexer().unwrap();
    let mut pool = models.load_indexer().unwrap().unwrap();
    let queries: Vec<_> = corpus.queries.iter().map(|q| q.text.as_str()).collect();
    // Fixed query vectors isolate document changes from query/batch variation.
    let query_vectors = vectors(&mut pool, &queries, Inference::Singleton);
    let raw: Vec<_> = corpus.documents.iter().map(|d| d.raw.as_str()).collect();
    let cleaned: Vec<_> = corpus
        .documents
        .iter()
        .map(|d| d.cleaned.as_str())
        .collect();
    let mut trials = Vec::new();
    for inference in [Inference::Singleton, Inference::DocumentBatch] {
        let start = Instant::now();
        let raw_vectors = vectors(&mut pool, &raw, inference);
        let raw_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let cleaned_vectors = vectors(&mut pool, &cleaned, inference);
        let cleaned_ms = start.elapsed().as_secs_f64() * 1000.0;
        let queries = corpus
            .queries
            .iter()
            .zip(&query_vectors)
            .enumerate()
            .map(|(index, (query, vector))| {
                let metrics = |vectors: &[Vec<f64>]| {
                    let ranking = rank(vector, vectors);
                    let relevant =
                        |index: usize| query.relevant.contains(&corpus.documents[index].id);
                    let first = ranking.iter().position(|index| relevant(*index)).unwrap() + 1;
                    let found = ranking
                        .iter()
                        .take(5)
                        .filter(|index| relevant(**index))
                        .count();
                    let recall = f64::from(u32::try_from(found).unwrap())
                        / f64::from(u32::try_from(query.relevant.len()).unwrap());
                    (first, recall)
                };
                let (raw_rank, raw_recall_at_five) = metrics(&raw_vectors);
                let (cleaned_rank, cleaned_recall_at_five) = metrics(&cleaned_vectors);
                Comparison {
                    query: index,
                    raw_rank,
                    cleaned_rank,
                    raw_recall_at_five,
                    cleaned_recall_at_five,
                }
            })
            .collect();
        trials.push(Trial {
            inference,
            raw_ms,
            cleaned_ms,
            queries,
            document_cosines: raw_vectors
                .iter()
                .zip(&cleaned_vectors)
                .map(|(a, b)| cosine(a, b))
                .collect(),
        });
    }
    let report = Report {
        version: 1,
        corpus_fingerprint: blake3::hash(&bytes).to_hex().to_string(),
        generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
        documents: corpus.documents.len(),
        changed_documents: corpus
            .documents
            .iter()
            .filter(|d| d.raw != d.cleaned)
            .count(),
        trials,
    };
    let output = PathBuf::from(std::env::var_os("MASH_TEST_CLEANUP_REPORT").unwrap());
    assert!(!output.exists(), "use a fresh explicit report path");
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
