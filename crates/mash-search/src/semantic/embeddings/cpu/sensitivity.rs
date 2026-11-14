//! Isolate raw inference, batch composition, worker routing and stored quantization.
use super::*;
use crate::semantic::embeddings::EmbeddingPoolBackend;
use crate::semantic::retrieval::quantize_vector;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;

const PHRASES: [&str; 6] = [
    "deadlock",
    "runtime",
    "runtime confirmation",
    "current uncommitted planning",
    "evidence boundaries",
    "hey",
];
const CENTER: &str = "runtime deadlock investigation and evidence from a recall audit";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Neighbors {
    Singleton,
    Short,
    Long,
}

#[derive(Serialize)]
struct Metrics {
    raw_max_component_difference: f64,
    raw_cosine_to_reference: f64,
    stored_cosine_to_reference: f64,
    raw_center_cosine: f64,
    stored_center_cosine: f64,
    changed_stored_components: usize,
}

#[derive(Serialize)]
struct Observation {
    phrase: &'static str,
    neighbors: Neighbors,
    batch_size: usize,
    position: usize,
    worker: usize,
    repeat: usize,
    inference_ms: f64,
    difference: Metrics,
}

#[derive(Serialize)]
struct Report {
    schema: u8,
    generation: String,
    center: &'static str,
    model_load_ms: f64,
    scheduler_max_component_difference: f64,
    singleton_wave_ms: f64,
    observations: Vec<Observation>,
}

fn cosine(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    assert_ne!(a.len(), 0);
    assert!(a.iter().chain(b).all(|v| v.is_finite()));
    let norm = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    let denominator = norm(a) * norm(b);
    assert!(denominator > 0.0);
    (a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>() / denominator).clamp(-1.0, 1.0)
}

fn stored(vector: &[f32]) -> Vec<f64> {
    quantize_vector(vector)
        .values
        .into_iter()
        .map(|v| f64::from(v.cast_signed()))
        .collect()
}

fn max_difference(a: &[f32], b: &[f32]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).abs())
        .fold(0.0, f64::max)
}

fn difference(reference: &[f32], actual: &[f32], center: &[f32]) -> Metrics {
    let raw = |v: &[f32]| v.iter().map(|v| f64::from(*v)).collect::<Vec<_>>();
    let (reference_raw, actual_raw, center_raw) = (raw(reference), raw(actual), raw(center));
    let (reference_stored, actual_stored, center_stored) =
        (stored(reference), stored(actual), stored(center));
    Metrics {
        raw_max_component_difference: max_difference(reference, actual),
        raw_cosine_to_reference: cosine(&reference_raw, &actual_raw),
        stored_cosine_to_reference: cosine(&reference_stored, &actual_stored),
        raw_center_cosine: cosine(&actual_raw, &center_raw),
        stored_center_cosine: cosine(&actual_stored, &center_stored),
        changed_stored_components: reference_stored
            .iter()
            .zip(&actual_stored)
            .filter(|(a, b)| (*a - *b).abs() >= 1.0)
            .count(),
    }
}

#[test]
#[ignore = "explicit installed CPU models and private report directory required"]
fn benchmark_label_embedding_sensitivity() {
    let root = PathBuf::from(std::env::var_os("MASH_TEST_MODELS_DIR").expect("installed models"));
    let output = PathBuf::from(
        std::env::var_os("MASH_TEST_TOPIC_REPORT_DIR").expect("explicit report directory"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("label-embedding-sensitivity.json"))
        .unwrap();
    let selected = crate::semantic::Models::new(root);
    selected.require_indexer().unwrap();
    let generation = crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned();
    let started = Instant::now();
    let mut pool = selected.load_indexer().unwrap().unwrap();
    let model_load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let EmbeddingPoolBackend::Cpu(models) = &mut pool.backend else {
        panic!("CPU model required");
    };
    let center = models[0].embed(vec![CENTER], None).unwrap().remove(0);
    let references = PHRASES
        .iter()
        .map(|phrase| models[0].embed(vec![*phrase], None).unwrap().remove(0))
        .collect::<Vec<_>>();
    let long = "search indexing metadata database history ".repeat(40);
    let mut observations = Vec::new();
    for (phrase, reference) in PHRASES.iter().zip(&references) {
        for (neighbors, size) in [
            (Neighbors::Singleton, 1),
            (Neighbors::Short, 2),
            (Neighbors::Long, 2),
            (Neighbors::Short, 8),
            (Neighbors::Long, 8),
        ] {
            for position in 0..size {
                let neighbor = match neighbors {
                    Neighbors::Singleton | Neighbors::Short => "search metadata",
                    Neighbors::Long => long.as_str(),
                };
                let mut texts = vec![neighbor; size];
                texts[position] = phrase;
                let mut previous: Option<Vec<f32>> = None;
                for repeat in 0..2 {
                    let started = Instant::now();
                    let vectors = models[0].embed(texts.clone(), None).unwrap();
                    let inference_ms = started.elapsed().as_secs_f64() * 1000.0;
                    assert_eq!(vectors.len(), size);
                    let actual = &vectors[position];
                    if let Some(previous) = &previous {
                        assert!(max_difference(previous, actual) <= 1e-5);
                    }
                    observations.push(Observation {
                        phrase,
                        neighbors,
                        batch_size: size,
                        position,
                        worker: 0,
                        repeat,
                        inference_ms,
                        difference: difference(reference, actual, &center),
                    });
                    previous = Some(actual.clone());
                }
            }
        }
        // A singleton must remain independent of worker and previous batch shape.
        for (worker, model) in models.iter_mut().enumerate() {
            let started = Instant::now();
            let actual = model.embed(vec![*phrase], None).unwrap().remove(0);
            assert!(max_difference(reference, &actual) <= 1e-5);
            observations.push(Observation {
                phrase,
                neighbors: Neighbors::Singleton,
                batch_size: 1,
                position: 0,
                worker,
                repeat: 2,
                inference_ms: started.elapsed().as_secs_f64() * 1000.0,
                difference: difference(reference, &actual, &center),
            });
        }
    }
    let scheduler_max_component_difference = scheduler_difference(models);
    let singleton_wave_ms = singleton_waves(&mut pool, &references);
    serde_json::to_writer_pretty(
        file,
        &Report {
            schema: 1,
            generation,
            center: CENTER,
            model_load_ms,
            scheduler_max_component_difference,
            singleton_wave_ms,
            observations,
        },
    )
    .unwrap();
}

fn singleton_waves(pool: &mut crate::semantic::EmbeddingPool, references: &[Vec<f32>]) -> f64 {
    let started = Instant::now();
    for order in [
        (0..64).map(|i| i % PHRASES.len()).collect::<Vec<_>>(),
        (0..64).rev().map(|i| i % PHRASES.len()).collect(),
        vec![5, 0, 2],
    ] {
        let texts = order.iter().map(|i| PHRASES[*i]).collect::<Vec<_>>();
        let actual = pool.singleton_label_vectors(&texts).unwrap();
        assert_eq!(actual.len(), order.len());
        for (index, actual) in order.iter().zip(&actual) {
            let expected = stored(&references[*index]);
            assert_eq!(&expected, actual);
        }
    }
    assert!(pool.singleton_label_vectors(&["test"; 65]).is_err());
    assert_eq!(pool.singleton_label_vectors(&[]).unwrap().len(), 0);
    started.elapsed().as_secs_f64() * 1000.0
}

fn scheduler_difference(models: &mut [TextEmbedding]) -> f64 {
    // Compare every output, not just the probe, with the identical sequential
    // batch composition. This distinguishes model sensitivity from reordering.
    let groups = (0..64)
        .map(|i| EmbeddingGroup {
            content: format!("{} marker {i}", PHRASES[i % PHRASES.len()]),
            message_ids: Vec::new(),
        })
        .collect::<Vec<_>>();
    let expected = groups
        .chunks(EMBEDDING_BATCH_SIZE)
        .flat_map(|batch| {
            models[0]
                .embed(
                    batch.iter().map(|g| g.content.as_str()).collect::<Vec<_>>(),
                    None,
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    let actual = embed_wave(models, &groups).unwrap();
    assert_eq!(actual.len(), expected.len());
    let scheduler_max_component_difference = expected
        .iter()
        .zip(&actual)
        .map(|(a, b)| max_difference(a, b))
        .fold(0.0, f64::max);
    assert!(scheduler_max_component_difference <= 1e-5);
    scheduler_max_component_difference
}
