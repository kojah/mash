use super::*;

#[test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
fn reranker_cpu_scores_and_empty_inputs_preserve_contract() {
    let root = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap();
    let selected = super::super::generations::select(Path::new(&root), false).unwrap();
    let mut model = Reranker::load(&selected.root, false).unwrap();
    assert_eq!(model.run("query", &[]).unwrap().len(), 0);
    let documents = ["vector search in Qdrant", "garden carrots", "good morning"];
    let expected = model
        .cpu
        .rerank("vector search", documents, false, Some(4))
        .unwrap();
    let actual = model.run("vector search", &documents).unwrap();
    assert_eq!(actual.len(), expected.len());
    for (expected, actual) in expected.iter().zip(&actual) {
        assert_eq!(expected.index, actual.index);
        assert!((expected.score - actual.score).abs() < 0.001);
    }
    assert_eq!(model.engine(), RerankerEngine::Cpu);
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    assert!(model.accelerated.is_none());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires Apple Silicon and explicitly installed MASH_TEST_MODELS_DIR"]
fn coreml_reranker_matches_cpu_across_shapes_and_measures_warm_latency() {
    let root = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap();
    let selected = super::super::generations::select(Path::new(&root), false).unwrap();
    let mut model = Reranker::load(&selected.root, true).unwrap();
    let query = "vector search";
    for repetitions in [1, 12, 24, 60, 140, 700] {
        let documents = [
            "Qdrant provides vector search. ".repeat(repetitions),
            "I grow carrots in my garden. ".repeat(repetitions),
            "Semantic retrieval searches embeddings. ".repeat(repetitions),
            "How is the weather today? ".repeat(repetitions),
            "We index chat history for semantic search. ".repeat(repetitions),
        ];
        let refs = documents.iter().map(String::as_str).collect::<Vec<_>>();
        for count in [1, 3, 4, 5] {
            let expected = model
                .cpu
                .rerank(query, &refs[..count], false, Some(4))
                .unwrap();
            let actual = model.run(query, &refs[..count]).unwrap();
            assert_eq!(
                model.engine(),
                RerankerEngine::CoreMl,
                "must not benchmark CPU fallback"
            );
            assert_eq!(actual.len(), count);
            for expected in &expected {
                let actual = actual
                    .iter()
                    .find(|actual| actual.index == expected.index)
                    .unwrap();
                assert!(
                    (actual.score - expected.score).abs() < 0.001,
                    "length={repetitions} count={count}: expected={expected:?} actual={actual:?}"
                );
            }
        }
        let mut cpu = Vec::new();
        let mut coreml = Vec::new();
        // Alternate order, after compilation and warmup, on identical inputs.
        for iteration in 0..10 {
            for accelerate in if iteration % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let start = std::time::Instant::now();
                if accelerate {
                    model.run(query, &refs[..4]).unwrap();
                    coreml.push(start.elapsed().as_micros());
                } else {
                    model.cpu.rerank(query, &refs[..4], false, Some(4)).unwrap();
                    cpu.push(start.elapsed().as_micros());
                }
            }
        }
        cpu.sort_unstable();
        coreml.sort_unstable();
        println!(
            "COREML_RERANK repetitions={repetitions} cpu_median_us={} coreml_median_us={}",
            cpu[5], coreml[5]
        );
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires Apple Silicon and explicitly installed MASH_TEST_MODELS_DIR"]
fn coreml_reranker_failure_uses_cpu_and_does_not_retry_acceleration() {
    let root = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap();
    let selected = super::super::generations::select(Path::new(&root), false).unwrap();
    let mut model = Reranker::load(&selected.root, true).unwrap();
    let missing = tempfile::tempdir().unwrap();
    model.accelerated = Some(crate::semantic::coreml::reranker::Pool::new(
        missing.path(),
        model.cpu.tokenizer.clone(),
    ));
    for _ in 0..2 {
        let actual = model
            .run("vector search", &["vector search", "carrots"])
            .unwrap();
        assert_eq!(actual.len(), 2);
        assert_eq!(actual[0].index, 0);
        assert_eq!(model.engine(), RerankerEngine::CpuFallback);
        assert!(model.accelerated.is_none());
    }
}
