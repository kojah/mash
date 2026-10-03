//! Output of the upstream compressor, hot-restart and recording pipeline.
use super::*;

const CAPTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/docs/fixtures/gemini-pipeline-38700b4.jsonl"
));

#[test]
fn native_compression_pipeline_preserves_original_dialogue_without_synthetic_copies() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("session-pipeline.jsonl");
    std::fs::write(&source, CAPTURE).unwrap();
    let replay = replay(&source).unwrap();
    assert_eq!(replay.archived.len(), 4);
    assert_eq!(
        replay
            .archived
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["early-user", "early-answer", "later-user", "later-answer"]
    );
    // Captured first-party loader retains only these three active turns.
    assert_eq!(
        replay
            .messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["pipeline-3", "pipeline-4", "final-user"]
    );
    let parsed = parse(&source).unwrap().conversation.unwrap();
    let searchable = parsed
        .messages
        .inline()
        .iter()
        .filter_map(|m| m.dialogue_text.as_deref().filter(|p| !p.is_empty()))
        .collect::<Vec<_>>();
    assert_eq!(
        searchable,
        [
            "How should the cBioPortal mutation importer validate metadata?",
            "Reject incompatible reference genomes before importing mutations.",
            "Also validate structural variants and chromosome aliases.",
            "Normalize chromosome aliases before checking structural variants.",
            "Keep the importer changes limited to validation.",
        ]
    );
    assert_eq!(parsed.messages.len(), 7);

    // Original message identities survive actual UUID reassignment on restart.
    let mut prefix = String::new();
    for line in CAPTURE.lines() {
        prefix.push_str(line);
        prefix.push('\n');
        let record: Record = serde_json::from_str(line).unwrap();
        if matches!(record, Record::Set { update } if update.messages.is_some()) {
            break;
        }
    }
    let before = directory.path().join("session-before.jsonl");
    std::fs::write(&before, prefix).unwrap();
    let before = parse(&before).unwrap().conversation.unwrap();
    for index in 0..2 {
        assert_eq!(
            parsed.messages.inline()[index].id,
            before.messages.inline()[index].id
        );
    }
    assert_eq!(std::fs::read_to_string(&source).unwrap(), CAPTURE);
}

#[test]
fn native_pipeline_index_excludes_failed_compression_rollback_and_rewind_payloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("session-pipeline.jsonl");
    std::fs::write(&source, CAPTURE).unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    index(&mut storage, &options(&source)).unwrap();
    storage.commit_writer().unwrap();
    for query in [
        "cBioPortal",
        "reference genomes",
        "structural variants",
        "chromosome aliases",
        "limited to validation",
    ] {
        assert!(
            !storage.search(query, 10, None, None).unwrap().is_empty(),
            "{query}"
        );
    }
    for query in [
        "abortpipelinecanary",
        "abortanswerpipelinecanary",
        "cancelpipelinecanary",
        "rewindpipelinecanary",
        "inflationpipelinecanary",
        "summarypipelinecanary",
        "secondsummarypipelinecanary",
        "Thanks for the additional context",
    ] {
        assert!(
            storage.search(query, 10, None, None).unwrap().is_empty(),
            "{query}"
        );
    }
}
