use super::*;
use crate::semantic::shared_query_vector;

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
    let marker = Models::new(source.clone()).read_valid_marker().unwrap();
    for file in &marker.files {
        let from = source.join(&file.path);
        let to = root.join(&file.path);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::hard_link(&from, &to)
            .or_else(|_| fs::copy(&from, &to).map(|_| ()))
            .unwrap();
    }
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
