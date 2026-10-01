//! Read-only projection inventory; not a clustering-quality benchmark.
use super::Policy;
use serde::Serialize;

#[derive(Serialize)]
struct Change {
    thread: String,
    retained_before: usize,
    retained_after: usize,
    omitted_skill_documents: usize,
}
#[derive(Serialize)]
struct Inventory {
    schema_version: u32,
    scope: &'static str,
    total_threads: usize,
    newly_empty_threads: usize,
    omitted_skill_documents: usize,
    unchanged_replacement_texts: bool,
    changed: Vec<Change>,
}

#[test]
#[ignore = "manual read-only corpus inventory requires explicit database and private output directory"]
fn benchmark_skill_context_inventory() {
    let path = std::path::PathBuf::from(
        std::env::var_os("MASH_TEST_SEARCH_DB").expect("explicit read-only database"),
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("MASH_TEST_TOPIC_REPORT_DIR").expect("explicit private report directory"),
    );
    let (threads, _) = crate::topics::evaluation::corpus(&path);
    let before = super::super::prepare(&threads, Policy::Preserve, None).unwrap();
    let after = super::super::prepare(&threads, Policy::OmitSkillDocuments, None).unwrap();
    assert_eq!(
        before.texts, after.texts,
        "exclusions do not rewrite or embed text"
    );
    let mut changed = Vec::new();
    let mut newly_empty_threads = 0;
    let mut total_omitted = 0;
    for (before, after) in before.plans.iter().zip(&after.plans) {
        assert_eq!(before.thread, after.thread);
        assert_eq!(before.projected.len(), after.projected.len());
        if after.omitted_skill_documents == 0 {
            assert_eq!(before.sources.len(), after.sources.len());
            continue;
        }
        assert!(before.sources.len() >= after.sources.len());
        total_omitted += after.omitted_skill_documents;
        newly_empty_threads += usize::from(!before.sources.is_empty() && after.sources.is_empty());
        changed.push(Change {
            thread: after.thread.clone(),
            retained_before: before.sources.len(),
            retained_after: after.sources.len(),
            omitted_skill_documents: after.omitted_skill_documents,
        });
    }
    let report = Inventory {
        schema_version: 1,
        scope: "derived_topic_input_only",
        total_threads: threads.len(),
        newly_empty_threads,
        omitted_skill_documents: total_omitted,
        unchanged_replacement_texts: true,
        changed,
    };
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join("skill-context-inventory.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
