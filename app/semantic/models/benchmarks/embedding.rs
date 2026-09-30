//! Isolated CPU embedding budgets on a frozen, explicitly prepared pending set.
use super::*;
use crate::semantic::embeddings::plan_embedding_groups;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Reference {
    digest: String,
    vector: Vec<f32>,
}

#[derive(Serialize)]
struct Sample {
    workers: usize,
    threads: usize,
    batch_size: usize,
    cpu_arena: bool,
    policy: Policy,
    documents: usize,
    model_load_milliseconds: f64,
    inference_milliseconds: f64,
    max_vector_difference: f32,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Policy {
    Parallel,
    Bounded,
}

fn infer_wave(
    models: &mut [TextEmbedding],
    wave: &[crate::semantic::embeddings::EmbeddingGroup],
    batch_size: usize,
    policy: Policy,
) -> Vec<Vec<f32>> {
    if matches!(policy, Policy::Bounded) {
        assert_eq!(models.len(), 8);
        assert_eq!(batch_size, 8);
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        return crate::semantic::embeddings::cpu::embed_wave(models, wave).unwrap();
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        panic!("bounded CPU policy is not used by native Core ML indexing");
    }
    std::thread::scope(|scope| {
        models
            .iter_mut()
            .zip(wave.chunks(batch_size))
            .map(|(model, batch)| {
                scope.spawn(move || {
                    model
                        .embed(
                            batch.iter().map(|g| g.content.as_str()).collect::<Vec<_>>(),
                            Some(batch_size),
                        )
                        .unwrap()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    })
}

fn budget(name: &str, default: usize, limit: usize) -> usize {
    let value = std::env::var(name).map_or(default, |v| v.parse().unwrap());
    assert!((1..=limit).contains(&value), "invalid {name}");
    value
}

#[test]
#[ignore = "manual CPU memory benchmark requires frozen pending index, models and vector reference"]
fn benchmark_embedding_worker_budget() {
    let (selected, storage, _) = fixture();
    let groups = plan_embedding_groups(storage.messages_needing_embeddings().unwrap());
    assert!(!groups.is_empty(), "prepare a frozen pending embedding set");
    let workers = budget("MASH_BENCH_EMBED_WORKERS", 8, 8);
    let threads = budget("MASH_BENCH_EMBED_THREADS", 2, 16);
    let batch_size = budget("MASH_BENCH_EMBED_BATCH", 8, 8);
    let policy =
        std::env::var("MASH_BENCH_EMBED_POLICY").map_or(Policy::Parallel, |v| match v.as_str() {
            "parallel" => Policy::Parallel,
            "bounded" => Policy::Bounded,
            _ => panic!("invalid MASH_BENCH_EMBED_POLICY"),
        });
    let cpu_arena = std::env::var("MASH_BENCH_EMBED_ARENA").map_or(true, |v| match v.as_str() {
        "true" => true,
        "false" => false,
        _ => panic!("invalid MASH_BENCH_EMBED_ARENA"),
    });
    let started = Instant::now();
    let mut models = (0..workers)
        .map(|_| {
            let options = InitOptionsUserDefined::default()
                .with_intra_threads(threads)
                .with_execution_providers(vec![
                    ort::ep::CPU::default()
                        .with_arena_allocator(cpu_arena)
                        .build(),
                ]);
            load_local_embedding_with_options(&selected.root, options).unwrap()
        })
        .collect::<Vec<_>>();
    let model_load_milliseconds = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let mut actual = Vec::new();
    for wave in groups.chunks(workers * batch_size) {
        let vectors = infer_wave(&mut models, wave, batch_size, policy);
        assert_eq!(vectors.len(), wave.len());
        actual.extend(wave.iter().zip(vectors).map(|(group, vector)| Reference {
            digest: blake3::hash(group.content.as_bytes()).to_hex().to_string(),
            vector,
        }));
    }
    let inference_milliseconds = started.elapsed().as_secs_f64() * 1000.0;
    let reference_path = PathBuf::from(
        std::env::var_os("MASH_BENCH_EMBED_REFERENCE").expect("explicit reference path"),
    );
    let max_vector_difference = if reference_path.exists() {
        let reference: Vec<Reference> =
            serde_json::from_slice(&std::fs::read(&reference_path).unwrap()).unwrap();
        assert_eq!(reference.len(), actual.len());
        reference
            .iter()
            .zip(&actual)
            .map(|(expected, actual)| {
                assert_eq!(expected.digest, actual.digest);
                assert_eq!(expected.vector.len(), actual.vector.len());
                expected
                    .vector
                    .iter()
                    .zip(&actual.vector)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max)
            })
            .fold(0.0_f32, f32::max)
    } else {
        std::fs::write(reference_path, serde_json::to_vec(&actual).unwrap()).unwrap();
        0.0
    };
    assert!(
        max_vector_difference <= 1e-5,
        "embedding vectors changed: {max_vector_difference}"
    );
    println!(
        "{}",
        serde_json::to_string(&Sample {
            workers,
            threads,
            batch_size,
            cpu_arena,
            policy,
            documents: actual.len(),
            model_load_milliseconds,
            inference_milliseconds,
            max_vector_difference,
        })
        .unwrap()
    );
}
