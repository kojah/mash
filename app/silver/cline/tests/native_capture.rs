//! Frozen artifacts written by pinned first-party helpers, not a full UI run.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Capture {
    scope: String,
    commit: String,
    artifact_sha256: BTreeMap<String, String>,
    expected_contents: Vec<String>,
}

impl Capture {
    fn assert_native_display(&self, root: &Path) {
        let rendered = parsed(&root.join("display-merged/tasks/task-1/ui_messages.json"));
        assert_eq!(rendered.messages.len(), 10);
        assert_eq!(
            rendered
                .messages
                .inline()
                .iter()
                .map(|message| message.dialogue_text.as_deref().unwrap())
                .collect::<Vec<_>>(),
            self.expected_contents
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );
        assert!(
            rendered.messages.inline()[6]
                .content
                .contains("<user_input")
        );
        assert_ne!(
            rendered.messages.inline()[6].id,
            rendered.messages.inline()[8].id
        );
    }

    fn install_phase(&self, fixtures: &Path, root: &Path, phase: &str) -> IndexOptions {
        for (file, expected_hash) in &self.artifact_sha256 {
            let Ok(relative) = Path::new(file).strip_prefix(phase) else {
                continue;
            };
            let bytes = std::fs::read(fixtures.join(file)).unwrap();
            assert_eq!(hex::encode(Sha256::digest(&bytes)), *expected_hash);
            let destination = root.join(relative);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::write(destination, bytes).unwrap();
        }
        IndexOptions {
            providers: [(Provider::Cline, vec![root.join("legacy"), root.join("sdk")])].into(),
            roots_are_authoritative: true,
            since_days: None,
        }
    }
}

#[test]
fn first_party_migration_artifacts_preserve_projection_and_persisted_ids_after_compaction() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("app/silver/cline/tests/fixtures/native-helper");
    let capture: Capture =
        serde_json::from_slice(&std::fs::read(fixtures.join("capture.json")).unwrap()).unwrap();
    assert_eq!(capture.scope, "cline-first-party-helper-control-v2");
    assert_eq!(capture.commit, "3435f72fcf4cb843bee946b8f9e981683564c9e3");
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("mash.sqlite3");
    let mut previous = None;
    for phase in ["initial", "compacted", "regenerated"] {
        let options = capture.install_phase(&fixtures, directory.path(), phase);
        capture.assert_native_display(directory.path());
        let scans = direct_scans(&options).unwrap();
        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].sources.len(), 1);
        let source = &scans[0].sources[0];
        let conversation = crate::silver::DirectScan::parse_source(
            Provider::Cline,
            source,
            &source.stamp().unwrap(),
        )
        .unwrap()
        .conversation
        .unwrap();
        let anchor = conversation.messages.inline()[0].id.clone();
        let mut storage = Storage::open_writer(&database).unwrap();
        index(&mut storage, &options).unwrap();
        storage.commit_writer().unwrap();
        drop(storage);
        let storage = Storage::open(&database).unwrap();
        assert_eq!(storage.counts().unwrap().conversations, 1);
        assert_eq!(storage.counts().unwrap().messages, 10);
        let visible = storage
            .view_content(
                &anchor,
                10,
                crate::storage::MessageContent::SearchProjection,
            )
            .unwrap();
        assert_eq!(
            visible
                .iter()
                .map(|message| &message.content)
                .collect::<Vec<_>>(),
            capture.expected_contents.iter().collect::<Vec<_>>()
        );
        assert_ne!(
            visible[6].id, visible[8].id,
            "repeated user turns stay distinct"
        );
        assert_eq!(visible[6].role, Role::User);
        assert_eq!(visible[8].role, Role::User);
        let canonical = storage.view(&anchor, 10).unwrap();
        assert!(
            canonical[6].content.contains("<user_input"),
            "canonical viewing preserves the native envelope"
        );
        assert!(canonical.iter().all(|message| {
            !message.content.contains("compaction-only-canary")
                && !message
                    .content
                    .contains(super::super::migration::RESUME_WARNING)
        }));
        let snapshot = canonical
            .iter()
            .map(|message| {
                (
                    message.id.clone(),
                    message.role.clone(),
                    message.content.clone(),
                )
            })
            .collect::<Vec<_>>();
        if let Some(previous) = &previous {
            assert_eq!(&snapshot, previous, "{phase}");
        }
        previous = Some(snapshot);
        assert!(
            storage
                .search("archived conversation", 10, None, None)
                .unwrap()
                .iter()
                .any(|hit| hit.content == capture.expected_contents[2]),
            "visible legacy turn excluded from model replay remains searchable"
        );
        assert!(
            storage
                .search("compaction-only-canary", 10, None, None)
                .unwrap()
                .is_empty()
        );
    }
}
