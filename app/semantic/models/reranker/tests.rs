use super::*;

#[test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
fn reranker_cpu_scores_and_empty_inputs_preserve_contract() {
    let root = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap();
    let selected = super::super::generations::select(Path::new(&root), false).unwrap();
    let mut model = Reranker::load(&selected.root).unwrap();
    assert_eq!(model.run("query", &[]).unwrap().len(), 0);
    let documents = ["vector search in Qdrant", "garden carrots", "good morning"];
    let actual = model.run("vector search", &documents).unwrap();
    assert_eq!(actual.len(), documents.len());
    assert_eq!(actual[0].index, 0, "the relevant document must rank first");
    assert_eq!(
        actual
            .iter()
            .map(|item| item.index)
            .collect::<std::collections::BTreeSet<_>>(),
        (0..documents.len()).collect()
    );
    assert!(actual.iter().all(|item| item.score.is_finite()));
    assert!(actual.windows(2).all(|pair| pair[0].score >= pair[1].score));
}
