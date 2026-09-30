//! Preserve quantization batches while bounding long-sequence workspaces.
use super::{AppError, EMBEDDING_BATCH_SIZE, EMBEDDING_WORKERS, EmbeddingGroup, TextEmbedding};

const LONG_WORKERS: usize = 2;
const SHORT_SEQUENCE_TOKENS: usize = 128;
const TOKENIZER_PREFLIGHT_BYTES: usize = 4096;

fn is_short_batch(model: &TextEmbedding, batch: &[EmbeddingGroup]) -> Result<bool, AppError> {
    // Avoid creating overflow encodings twice for a very large message. A
    // conservative classification changes scheduling, never model input.
    if batch
        .iter()
        .any(|g| g.content.len() > TOKENIZER_PREFLIGHT_BYTES)
    {
        return Ok(false);
    }
    let texts = batch.iter().map(|g| g.content.as_str()).collect::<Vec<_>>();
    let encodings = model
        .tokenizer
        .encode_batch(texts, true)
        .map_err(|error| AppError::model(format!("embedding tokenization failed: {error}")))?;
    Ok(encodings.iter().all(|e| e.len() <= SHORT_SEQUENCE_TOKENS))
}

pub(crate) fn embed_wave(
    workers: &mut [TextEmbedding],
    groups: &[EmbeddingGroup],
) -> Result<Vec<Vec<f32>>, AppError> {
    if workers.len() != EMBEDDING_WORKERS {
        return Err(AppError::internal("unexpected CPU embedding worker count"));
    }
    let mut jobs = (0..workers.len()).map(|_| Vec::new()).collect::<Vec<_>>();
    let (mut short, mut long) = (0, 0);
    for (index, batch) in groups.chunks(EMBEDDING_BATCH_SIZE).enumerate() {
        let worker = if is_short_batch(&workers[0], batch)? {
            let worker = (LONG_WORKERS + short) % workers.len();
            short += 1;
            worker
        } else {
            let worker = long % LONG_WORKERS;
            long += 1;
            worker
        };
        jobs[worker].push((index, batch));
    }
    let mut results = std::thread::scope(|scope| {
        workers
            .iter_mut()
            .zip(jobs)
            .map(|(worker, jobs)| {
                scope.spawn(move || {
                    jobs.into_iter()
                        .map(|(index, batch)| {
                            let texts =
                                batch.iter().map(|g| g.content.as_str()).collect::<Vec<_>>();
                            let vectors = worker.embed(texts, None).map_err(|error| {
                                AppError::model(format!("embedding inference failed: {error}"))
                            })?;
                            if vectors.len() != batch.len() {
                                return Err(AppError::model(
                                    "embedding model returned an unexpected result count",
                                ));
                            }
                            Ok((index, vectors))
                        })
                        .collect::<Result<Vec<_>, AppError>>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| AppError::internal("embedding worker panicked"))?
            })
            .collect::<Result<Vec<_>, _>>()
    })?
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    results.sort_unstable_by_key(|(index, _)| *index);
    Ok(results
        .into_iter()
        .flat_map(|(_, vectors)| vectors)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
    fn cpu_scheduler_preserves_quantized_batches_and_message_order() {
        let root = std::path::PathBuf::from(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap());
        let mut pool = crate::semantic::Models::new(root)
            .load_indexer()
            .unwrap()
            .unwrap();
        let super::super::EmbeddingPoolBackend::Cpu(models) = &mut pool.backend else {
            panic!("local CPU pool required");
        };
        let groups = (0..24)
            .map(|index| EmbeddingGroup {
                content: if (8..16).contains(&index) {
                    format!(
                        "long document {index} {}",
                        "search database history ".repeat(400)
                    )
                } else {
                    format!("short search document {index}")
                },
                message_ids: vec![format!("message-{index}")],
            })
            .collect::<Vec<_>>();
        assert!(is_short_batch(&models[0], &groups[..8]).unwrap());
        assert!(!is_short_batch(&models[0], &groups[8..16]).unwrap());
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
        assert_eq!(expected.len(), actual.len());
        for (expected, actual) in expected.iter().zip(&actual) {
            assert_eq!(expected.len(), actual.len());
            assert!(
                expected
                    .iter()
                    .zip(actual)
                    .all(|(a, b)| (a - b).abs() <= 1e-5)
            );
        }
        let dense = (0..8)
            .map(|_| EmbeddingGroup {
                content: "x! ".repeat(100),
                message_ids: Vec::new(),
            })
            .collect::<Vec<_>>();
        assert!(
            !is_short_batch(&models[0], &dense).unwrap(),
            "byte length cannot substitute for token length"
        );
        assert!(embed_wave(&mut models[..2], &groups).is_err());
    }
}
