//! Reviewed-source scope check, without model work or canonical mutation.
use super::{super::runtime_context, Selection};
use serde::Serialize;

#[derive(Serialize)]
struct Change {
    thread: String,
    retained_before: usize,
    retained_after: usize,
    omitted_startup_context: usize,
}

#[derive(Serialize)]
struct Inventory {
    schema_version: u32,
    startup_context_policy: super::Policy,
    total_threads: usize,
    omitted_startup_context: usize,
    newly_empty_threads: usize,
    unchanged_replacement_texts: bool,
    changed: Vec<Change>,
}

#[test]
#[ignore = "manual read-only inventory requires explicit corpus, reviewed manifest and private output"]
fn benchmark_reviewed_startup_inventory() {
    let database =
        std::path::PathBuf::from(std::env::var_os("MASH_TEST_SEARCH_DB").expect("explicit corpus"));
    let output = std::path::PathBuf::from(
        std::env::var_os("MASH_TEST_TOPIC_REPORT_DIR").expect("explicit private output"),
    );
    let selection = Selection::load()
        .unwrap()
        .expect("explicit reviewed manifest");
    let (threads, _) = crate::topics::evaluation::corpus(&database);
    let before =
        super::super::prepare(&threads, runtime_context::Policy::OmitSkillDocuments, None).unwrap();
    let after = super::super::prepare(
        &threads,
        runtime_context::Policy::OmitSkillDocuments,
        Some(&selection),
    )
    .unwrap();
    assert_eq!(before.texts, after.texts);
    let mut changed = Vec::new();
    let mut newly_empty_threads = 0;
    let mut omitted_startup_context = 0;
    for (before, after) in before.plans.iter().zip(&after.plans) {
        assert_eq!(before.thread, after.thread);
        assert_eq!(
            before.omitted_skill_documents,
            after.omitted_skill_documents
        );
        assert_eq!(before.projected.len(), after.projected.len());
        assert_eq!(
            before.sources.len(),
            after.sources.len() + after.omitted_startup_context
        );
        if after.omitted_startup_context == 0 {
            continue;
        }
        omitted_startup_context += after.omitted_startup_context;
        newly_empty_threads += usize::from(!before.sources.is_empty() && after.sources.is_empty());
        changed.push(Change {
            thread: after.thread.clone(),
            retained_before: before.sources.len(),
            retained_after: after.sources.len(),
            omitted_startup_context: after.omitted_startup_context,
        });
    }
    let report = Inventory {
        schema_version: 1,
        startup_context_policy: Selection::policy(Some(&selection)),
        total_threads: threads.len(),
        omitted_startup_context,
        newly_empty_threads,
        unchanged_replacement_texts: true,
        changed,
    };
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join("reviewed-startup-inventory.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
