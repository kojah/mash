//! Native recorder and setHistory capture: masking, regenerated IDs and rollback.
use super::*;
use sha2::{Digest, Sha256};

const MASKED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/docs/fixtures/gemini-tool-history-masked-38700b4.jsonl"
));
const FINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/docs/fixtures/gemini-tool-history-38700b4.jsonl"
));
const PROSE: [&str; 3] = [
    "Inspect cBioPortal importer metadata validation.",
    "I will inspect reference genomes and chromosome aliases.",
    "Validate structural variants before importing mutations.",
];

#[test]
fn native_tool_history_checkpoints_preserve_prose_and_follow_native_id_regeneration() {
    assert_eq!(
        hex::encode(Sha256::digest(MASKED)),
        "cc00ff7a44d88669a2fc19ab04970bc4b9b7b1746585e96112bb25d7b571f772"
    );
    assert_eq!(
        hex::encode(Sha256::digest(FINAL)),
        "ca2dbea56cf7bf62bddffe9c463aed2e2116b2473e8c33b71f0fb9d19a4b693e"
    );
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("session-tools.jsonl");
    for (capture, ids) in [
        (MASKED, ["request", "answer", "tool-result", "followup"]),
        (
            FINAL,
            [
                "tool-history-1",
                "tool-history-2",
                "tool-history-3",
                "tool-history-4",
            ],
        ),
    ] {
        std::fs::write(&source, capture).unwrap();
        let state = replay(&source).unwrap();
        assert!(
            state.archived.is_empty(),
            "masking is not evidence of summary compaction"
        );
        assert_eq!(
            state
                .messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ids
        );
        let parsed = parse(&source).unwrap().conversation.unwrap();
        assert_eq!(parsed.messages.len(), 4);
        assert_eq!(
            parsed
                .messages
                .inline()
                .iter()
                .filter_map(|m| m.search_projection.as_deref().filter(|p| !p.is_empty()))
                .collect::<Vec<_>>(),
            PROSE
        );
        assert_eq!(
            parsed.messages.inline()[2].search_projection.as_deref(),
            Some("")
        );
        assert!(
            parsed.messages.inline()[2]
                .content
                .contains("maskedtoolhistorycanary")
        );
        let answer = &parsed.messages.inline()[1].content;
        for context in ["argumenthistorycanary", "thought-history-canary"] {
            assert_eq!(
                answer.contains(context),
                capture == MASKED,
                "canonical metadata must follow the native snapshot"
            );
        }
        assert!(
            parsed
                .messages
                .inline()
                .iter()
                .all(|m| !m.content.contains("originaltoolhistorycanary"))
        );
        for (message, id) in parsed.messages.inline().iter().zip(ids) {
            assert_eq!(
                message.id,
                crate::ingestion::parsers::stable_id(
                    "message",
                    &[Provider::GeminiCli.as_str(), "tool-history-session", id]
                )
            );
        }
        assert_eq!(std::fs::read_to_string(&source).unwrap(), capture);
    }
}

#[test]
fn native_tool_mask_and_truncation_refresh_removes_stale_ids_without_losing_searchable_prose() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("session-tools.jsonl");
    let database = directory.path().join("index.sqlite3");
    let mut old_ids = Vec::new();
    for (capture, unchanged) in [(MASKED, false), (FINAL, false), (FINAL, true)] {
        if !unchanged {
            std::fs::write(&source, capture).unwrap();
        }
        let mut storage = Storage::open_writer(&database).unwrap();
        let summary = index(&mut storage, &options(&source)).unwrap();
        assert_eq!(summary.checkpoint_skipped_sources, u64::from(unchanged));
        if unchanged {
            assert_eq!(summary.changed_messages, 0);
        }
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 4);
        for query in [
            "metadata validation",
            "reference genomes",
            "chromosome aliases",
            "structural variants",
        ] {
            assert!(
                !storage.search(query, 10, None, None).unwrap().is_empty(),
                "{query}"
            );
        }
        for query in [
            "originaltoolhistorycanary",
            "maskedtoolhistorycanary",
            "argumenthistorycanary",
            "thought-history-canary",
            "cancelledhistorycanary",
            "rewoundhistorycanary",
        ] {
            assert!(
                storage.search(query, 10, None, None).unwrap().is_empty(),
                "{query}"
            );
        }
        let ids = parse(&source)
            .unwrap()
            .conversation
            .unwrap()
            .messages
            .inline()
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>();
        if capture == FINAL && !unchanged {
            assert!(ids.iter().all(|id| !old_ids.contains(id)));
            for old in &old_ids {
                assert!(
                    storage.view(old, 0).unwrap().is_empty(),
                    "old native identity survived authoritative replacement"
                );
            }
        }
        if unchanged {
            assert_eq!(ids, old_ids);
        }
        old_ids = ids;
    }
}
