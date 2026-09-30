use super::coreml::coreml_sequence_bucket;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::coreml::{CoreMlEmbeddingPool, load_coreml_embedding};
use super::embeddings::{EmbeddingGroup, plan_embedding_groups};
use super::retrieval::{quantize_vector, quantized_cosine, rank_quantized};
use super::*;
use crate::storage::{SearchHit, SearchableMessage, SemanticChunks};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::time::Instant;

#[test]
fn reranking_is_bounded_to_ten_documents() {
    assert_eq!(RERANK_LIMIT, 10);
}

#[test]
fn embedding_plan_groups_identical_text_and_sorts_message_ids() {
    let groups = plan_embedding_groups(vec![
        SearchableMessage {
            id: "message-c".to_owned(),
            content: "same text".to_owned(),
        },
        SearchableMessage {
            id: "message-b".to_owned(),
            content: "different".to_owned(),
        },
        SearchableMessage {
            id: "message-a".to_owned(),
            content: "same text".to_owned(),
        },
    ]);

    assert_eq!(groups.len(), 2);
    let repeated = groups
        .iter()
        .find(|group| group.content == "same text")
        .expect("repeated text group");
    assert_eq!(repeated.message_ids, ["message-a", "message-c"]);
}

#[test]
fn embedding_plan_is_length_ordered_and_input_order_independent() {
    fn contents(groups: &[EmbeddingGroup]) -> Vec<&str> {
        groups
            .iter()
            .map(|group| group.content.as_str())
            .collect::<Vec<_>>()
    }

    fn messages(order: &[usize]) -> Vec<SearchableMessage> {
        let values = [
            ("message-long", "longest text"),
            ("message-short-b", "b"),
            ("message-short-a", "a"),
            ("message-medium", "medium"),
        ];
        order
            .iter()
            .map(|index| SearchableMessage {
                id: values[*index].0.to_owned(),
                content: values[*index].1.to_owned(),
            })
            .collect()
    }

    let forward = plan_embedding_groups(messages(&[0, 1, 2, 3]));
    let reverse = plan_embedding_groups(messages(&[3, 2, 1, 0]));

    assert_eq!(contents(&forward), ["a", "b", "medium", "longest text"]);
    assert_eq!(contents(&forward), contents(&reverse));
}

#[test]
fn coreml_sequence_buckets_preserve_token_boundaries_and_maximum_length() {
    for (tokens, expected) in [
        (0, 64),
        (63, 64),
        (64, 64),
        (65, 128),
        (128, 128),
        (129, 256),
        (256, 256),
        (257, 512),
        (512, 512),
        (600, 512),
    ] {
        assert_eq!(coreml_sequence_bucket(tokens), expected);
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR and Apple Silicon Core ML"]
fn coreml_bucketed_batches_preserve_vectors_and_measure_throughput() {
    let root = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(PathBuf::from)
        .expect("explicit model directory");
    let mut pool = CoreMlEmbeddingPool::load(&root).unwrap();
    let mut reference = load_coreml_embedding(&root, COREML_SEQUENCE_LENGTH).unwrap();
    for words in [8, 80, 180, 400] {
        let groups = (0..COREML_BATCH_SIZE)
            .map(|i| EmbeddingGroup {
                content: format!(
                    "{} {i} punctuation, unicode café 日本語",
                    "hello ".repeat(words)
                ),
                message_ids: Vec::new(),
            })
            .collect::<Vec<_>>();
        let texts = groups
            .iter()
            .map(|g| g.content.as_str())
            .collect::<Vec<_>>();
        let expected = reference.embed(texts.clone(), None).unwrap();
        let actual = pool.embed(&groups).unwrap();
        let mut min_cosine = 1_f32;
        for (expected, actual) in expected.iter().zip(&actual) {
            let expected = quantize_vector(expected);
            let actual = quantize_vector(actual);
            let cosine = quantized_cosine(&expected, &actual.values, actual.norm);
            min_cosine = min_cosine.min(cosine);
            assert!(cosine >= 0.999, "{words} words: cosine {cosine}");
        }
        for _ in 0..2 {
            reference.embed(texts.clone(), None).unwrap();
            pool.embed(&groups).unwrap();
        }
        let start = Instant::now();
        for _ in 0..10 {
            reference.embed(texts.clone(), None).unwrap();
        }
        let reference_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        for _ in 0..10 {
            pool.embed(&groups).unwrap();
        }
        let bucket_ms = start.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{}",
            serde_json::json!({
                "words": words, "texts": 320, "fixed_512_ms": reference_ms,
                "bucketed_ms": bucket_ms, "speedup": reference_ms / bucket_ms,
                "min_quantized_cosine": min_cosine,
            })
        );
    }
    // Partial and empty waves preserve cardinality. Selection uses actual
    // tokens (including Unicode and special tokens), not character counts.
    assert_eq!(pool.embed(&[]).unwrap(), Vec::<Vec<f32>>::new());
    for content in ["brief status".to_owned(), "hello ".repeat(600)] {
        let groups = [EmbeddingGroup {
            content,
            message_ids: Vec::new(),
        }];
        let expected = reference
            .embed(vec![groups[0].content.as_str(); COREML_BATCH_SIZE], None)
            .unwrap();
        let actual = pool.embed(&groups).unwrap();
        assert_eq!(actual.len(), 1);
        let expected = quantize_vector(&expected[0]);
        let actual = quantize_vector(&actual[0]);
        assert!(quantized_cosine(&expected, &actual.values, actual.norm) >= 0.999);
    }
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn real_model_length_aware_batches_preserve_quantized_vectors() {
    let models = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(PathBuf::from)
        .expect("explicit model directory");
    let mut backend = Backend::load_local(&models).expect("load reference models");
    let mut pool = EmbeddingPool::load_local(&models).expect("load embedding pool");
    let samples = [
        "brief status".to_owned(),
        "a somewhat longer explanation of a database transaction boundary".to_owned(),
        "x ".repeat(300),
        "semantic retrieval with reciprocal rank fusion and bounded reranking".to_owned(),
    ];

    let reference = samples
        .iter()
        .map(|text| {
            let vector = backend
                .embed(&[text])
                .expect("reference embedding")
                .pop()
                .expect("reference vector");
            (text.clone(), quantize_vector(&vector))
        })
        .collect::<HashMap<_, _>>();
    let messages = samples
        .iter()
        .rev()
        .enumerate()
        .map(|(index, text)| crate::storage::SearchableMessage {
            id: format!("message-{index}"),
            content: text.clone(),
        })
        .collect();
    let groups = plan_embedding_groups(messages);
    let candidate = pool.embed_wave(&groups).expect("length-aware embeddings");
    let repeated = pool
        .embed_wave(&groups)
        .expect("repeated length-aware embeddings");

    for ((group, vector), repeated_vector) in groups.iter().zip(candidate).zip(repeated) {
        let actual = quantize_vector(&vector);
        let repeated = quantize_vector(&repeated_vector);
        let expected = &reference[&group.content];
        let similarity = quantized_cosine(expected, &actual.values, actual.norm);
        assert!(
            similarity >= 0.98,
            "text length {} had reference cosine {similarity}",
            group.content.len()
        );
        assert_eq!(actual.values, repeated.values, "{}", group.content);
        assert_eq!(actual.norm, repeated.norm, "{}", group.content);
    }
}

#[test]
fn quantized_cosine_handles_bounds_and_preserves_direction() {
    let positive = quantize_vector(&[1.0, 0.0]);
    let negative = quantize_vector(&[-1.0, 0.0]);
    let zero = quantize_vector(&[0.0, 0.0]);
    assert!(
        (quantized_cosine(&positive, &positive.values, positive.norm) - 1.0).abs() < f32::EPSILON
    );
    assert!(
        (quantized_cosine(&positive, &negative.values, negative.norm) + 1.0).abs() < f32::EPSILON
    );
    assert_eq!(quantized_cosine(&positive, &[127], 127.0), 0.0);
    assert_eq!(
        quantized_cosine(&zero, &positive.values, positive.norm),
        0.0
    );
}

#[test]
fn quantized_ranking_selects_the_best_candidates_deterministically() {
    let query = quantize_vector(&[1.0, 0.0]);
    let first = quantize_vector(&[1.0, 0.0]);
    let second = quantize_vector(&[0.8, 0.2]);
    let third = quantize_vector(&[-1.0, 0.0]);
    let vectors = SemanticChunks {
        chunks: vec![crate::storage::SemanticChunk {
            message_rowids: vec![10, 20, 30],
            values: [first.values, second.values, third.values].concat(),
            norms: vec![first.norm, second.norm, third.norm],
            eligible: vec![true; 3],
            dimensions: 2,
        }],
        dimensions: 2,
    };

    let ranked = rank_quantized(&query, &vectors, 2);
    assert_eq!(
        ranked.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        [10, 20]
    );
    assert!(ranked[0].1 > ranked[1].1);
}

#[test]
fn models_are_not_ready_without_a_valid_marker() {
    let directory = tempfile::tempdir().expect("temporary model directory");
    let models = Models::new(directory.path().to_path_buf());
    assert!(!models.is_installed());
    fs::write(directory.path().join(MARKER_NAME), b"not json").expect("invalid marker");
    assert!(!models.is_installed());
}

#[test]
fn rrf_fusion_rewards_overlap_and_breaks_ties_by_id() {
    fn hit(id: &str) -> SearchHit {
        SearchHit {
            id: id.to_owned(),
            conversation_id: "conversation".to_owned(),
            provider: crate::provider::Provider::Codex,
            role: crate::role::Role::User,
            content: id.to_owned(),
            lexical_score: None,
            semantic_score: None,
            fusion_score: 0.0,
            rerank_score: None,
            node_id: None,
        }
    }

    let mut semantic_b = hit("b");
    semantic_b.semantic_score = Some(0.8);
    let fused = fuse(vec![hit("a"), hit("b")], vec![hit("c"), semantic_b]);
    let ids: Vec<&str> = fused
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect();
    assert_eq!(ids, ["b", "a", "c"]);
    assert!(fused[0].fusion_score > fused[1].fusion_score);
    assert_eq!(fused[0].semantic_score, Some(0.8));
    assert_eq!(fused[1].fusion_score, fused[2].fusion_score);
}
