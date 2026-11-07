use super::assets::write_marker;
use super::*;
use crate::semantic::shared_query_vector;

#[test]
fn both_rankings_require_local_model_assets_without_creating_them() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("absent-models");
    let models = Models::new(path.clone());
    assert!(models.require_installed().is_err());
    for ranking in [
        crate::views::config::Ranking::Fusion,
        crate::views::config::Ranking::ContextualRerank,
    ] {
        assert!(models.load_search(ranking).unwrap().is_none());
    }
    assert!(!path.exists());
}

#[test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
fn ranking_selects_only_required_models_and_restores_contextual_backend() {
    let root = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into();
    let models = Models::new(root);
    for ranking in [
        crate::views::config::Ranking::Fusion,
        crate::views::config::Ranking::Fusion,
        crate::views::config::Ranking::ContextualRerank,
        crate::views::config::Ranking::Fusion,
    ] {
        let started = std::time::Instant::now();
        let mut backend = models.load_search(ranking).unwrap().unwrap();
        println!(
            "MODEL_SELECTION {ranking:?} load_us={} reranker_loaded={}",
            started.elapsed().as_micros(),
            backend.reranker.is_some()
        );
        assert_eq!(
            backend.reranker.is_some(),
            ranking == crate::views::config::Ranking::ContextualRerank
        );
        assert_ne!(
            shared_query_vector(&mut backend, "model cache")
                .unwrap()
                .len(),
            0
        );
        if ranking == crate::views::config::Ranking::ContextualRerank {
            assert_eq!(backend.rerank("cache", &["model cache"]).unwrap().len(), 1);
        } else {
            assert!(backend.rerank("cache", &["model cache"]).is_err());
        }
    }
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn query_focused_reranking_preserves_paraphrases_and_reveals_late_evidence() {
    let source = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(PathBuf::from)
        .expect("explicit model directory");
    let mut backend = Models::new(source).load().unwrap().unwrap();
    for (query, relevant) in [
        (
            "cbioportal",
            "Are the cbioportal-related skills transferred?",
        ),
        (
            "postgres redis performance",
            "Why is Redis search faster than PostgreSQL?",
        ),
        (
            "initial indexing progress",
            "The initial index should be explicit and display a progress monitor with remaining ETA.",
        ),
    ] {
        let documents = [
            relevant,
            "hello!",
            "sounds good keep going",
            "I planted carrots in my garden.",
        ];
        let scores = backend.rerank_search(query, &documents).unwrap();
        let relevant = scores
            .iter()
            .find(|result| result.index == 0)
            .unwrap()
            .score;
        assert!(
            scores
                .iter()
                .filter(|result| result.index != 0)
                .all(|result| result.score < relevant),
            "query {query}: {scores:?}"
        );
    }
    let long = format!(
        "{}cBioPortal importer",
        "unrelated introduction ".repeat(500)
    );
    let original = backend.rerank("cbioportal", &[long.as_str()]).unwrap()[0].score;
    let focused = backend
        .rerank_search("cbioportal", &[long.as_str()])
        .unwrap()[0]
        .score;
    assert!(focused > original, "original {original}, focused {focused}");
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn small_rerank_batches_preserve_scores_and_document_indices() {
    let source = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(PathBuf::from)
        .expect("explicit model directory");
    let mut backend = Models::new(source).load().unwrap().unwrap();
    let long = "unrelated gardening instructions ".repeat(1200);
    let documents = [
        "cBioPortal cancer genomics importer",
        long.as_str(),
        "please review the cBioPortal import bug",
        "hello",
    ];
    let expected = backend
        .reranker
        .as_mut()
        .unwrap()
        .cpu
        .rerank("cbioportal importer", documents, false, None)
        .unwrap();
    let actual = backend.rerank("cbioportal importer", &documents).unwrap();
    assert_eq!(actual.len(), expected.len());
    for (expected, actual) in expected.iter().zip(actual) {
        assert_eq!(actual.index, expected.index);
        assert!((actual.score - expected.score).abs() < 1e-4);
    }
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn cached_local_models_preserve_results_and_validate_changed_assets() {
    let source = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(PathBuf::from)
        .expect("explicit model directory");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let selected = generations::select(&source).unwrap();
    let mut marker = selected.marker;
    for file in &marker.files {
        let from = selected.root.join(&file.path);
        let to = root.join(&file.path);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::hard_link(&from, &to)
            .or_else(|_| fs::copy(&from, &to).map(|_| ()))
            .unwrap();
    }
    marker.asset_directory = None;
    write_marker(root, &marker).unwrap();
    let models = Models::new(root.to_path_buf());
    let query = "local search cache validation";
    let mut first = models.load().unwrap().unwrap();
    let expected_vector = shared_query_vector(&mut first, query).unwrap();
    let expected_scores = first
        .rerank(query, &["local search", "unrelated result"])
        .unwrap();
    drop(first);
    assert_eq!(
        LOCAL_BACKEND.lock().unwrap().as_ref().unwrap().key.root,
        root
    );
    let mut reused = models.load().unwrap().unwrap();
    assert!(LOCAL_BACKEND.lock().unwrap().is_none());
    assert_eq!(
        shared_query_vector(&mut reused, query).unwrap(),
        expected_vector
    );
    let actual = reused
        .rerank(query, &["local search", "unrelated result"])
        .unwrap();
    assert_eq!(actual.len(), expected_scores.len());
    for (expected, actual) in expected_scores.iter().zip(actual) {
        assert_eq!(expected.index, actual.index);
        assert!((expected.score - actual.score).abs() < 1e-5);
    }
    drop(reused);
    fs::File::open(root.join(MARKER_NAME))
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH)
        .unwrap();
    let refreshed = models.load().unwrap().unwrap();
    assert_eq!(
        refreshed.cache_key.as_ref().unwrap().marker_modified,
        Some(SystemTime::UNIX_EPOCH)
    );
    drop(refreshed);
    // Remove only the fixture's link; the explicitly installed assets remain intact.
    let asset = root.join(
        &marker
            .files
            .iter()
            .find(|file| file.size > 1024)
            .unwrap()
            .path,
    );
    fs::remove_file(&asset).unwrap();
    assert!(models.load().is_err());
    fs::write(&asset, b"incomplete replacement").unwrap();
    assert!(models.load().is_err());
    fs::remove_file(root.join(MARKER_NAME)).unwrap();
    assert!(models.load().unwrap().is_none());
    LOCAL_BACKEND.lock().unwrap().take();
}
