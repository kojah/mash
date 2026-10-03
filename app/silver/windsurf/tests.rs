use super::super::{IndexOptions, direct_scans, index};
use super::*;
use crate::storage::Storage;
use base64::{Engine, engine::general_purpose::STANDARD};
use prost::Message;
use serde::Deserialize;
use std::path::PathBuf;

mod retrieval;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    native_written_envelope_base64: String,
    native_written_protobuf_base64: String,
}
const ID: &str = "40000000-0000-4000-8000-000000000001";
fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../../docs/fixtures/windsurf-native-runtime-3.10.31.json"
    ))
    .unwrap()
}
fn write(root: &Path) -> PathBuf {
    let path = root.join(format!("{ID}.pb"));
    std::fs::write(
        &path,
        STANDARD
            .decode(fixture().native_written_envelope_base64)
            .unwrap(),
    )
    .unwrap();
    path
}
fn trajectory() -> proto::Trajectory {
    proto::Trajectory::decode(
        STANDARD
            .decode(fixture().native_written_protobuf_base64)
            .unwrap()
            .as_slice(),
    )
    .unwrap()
}
fn save(path: &Path, root: &proto::Trajectory) {
    std::fs::write(path, reader::seal(&root.encode_to_vec())).unwrap();
}
fn conversation(path: &Path) -> Conversation {
    parse(path).unwrap().conversation.unwrap()
}
fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Windsurf, vec![path.into()])].into(),
        since_days: None,
        roots_are_authoritative: true,
    }
}

#[test]
fn native_go_written_fixture_keeps_repeated_requests_and_separates_thinking() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path());
    let parsed = conversation(&path);
    assert_eq!(parsed.provider, Provider::Windsurf);
    assert_eq!(parsed.title.as_deref(), Some("Native writer probe"));
    assert_eq!(parsed.messages.len(), 6);
    assert_eq!(
        parsed
            .messages
            .inline()
            .iter()
            .filter(|m| !m.dialogue_text.as_deref().unwrap().is_empty())
            .count(),
        5
    );
    assert_ne!(
        parsed.messages.inline()[0].id,
        parsed.messages.inline()[2].id
    );
    assert_eq!(
        parsed.messages.inline()[0].content,
        parsed.messages.inline()[2].content
    );
    assert!(
        parsed.messages.inline()[1]
            .content
            .contains("Internal reasoning")
    );
    assert!(
        !parsed.messages.inline()[1]
            .dialogue_text
            .as_deref()
            .unwrap()
            .contains("Internal reasoning")
    );
    assert!(
        parsed.messages.inline()[5]
            .content
            .contains("Reasoning without")
    );
    assert_eq!(
        parsed.messages.inline()[5].dialogue_text.as_deref(),
        Some("")
    );
    // The pinned native transcript includes this cleared step too.
    assert_ne!(
        parsed.messages.inline()[4]
            .dialogue_text
            .as_deref()
            .unwrap(),
        ""
    );
    assert_eq!(parsed.created_at, Some(1_790_762_400_000));
    assert_eq!(parsed.updated_at, Some(1_790_762_406_000));
}

#[test]
fn native_snapshot_rewind_removes_suffix_without_changing_prefix_identities() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path());
    let before = conversation(&path);
    let mut root = trajectory();
    root.steps.truncate(2);
    root.steps[0].user.as_mut().unwrap().query = "Edited request".into();
    save(&path, &root);
    let after = conversation(&path);
    assert_eq!(after.messages.len(), 2);
    assert_eq!(
        before.messages.inline()[0].id,
        after.messages.inline()[0].id
    );
    assert_eq!(
        before.messages.inline()[1].id,
        after.messages.inline()[1].id
    );
    assert_eq!(
        after.messages.inline()[0].dialogue_text.as_deref(),
        Some("Edited request")
    );
}

#[test]
fn system_model_copied_unknown_and_background_prompts_are_context_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path());
    for origin in [
        proto::StepSource::System as i32,
        proto::StepSource::Model as i32,
        proto::StepSource::UserImplicit as i32,
        999,
    ] {
        let mut root = trajectory();
        root.steps[0].metadata.as_mut().unwrap().source = origin;
        save(&path, &root);
        assert_eq!(
            conversation(&path).messages.inline()[0]
                .dialogue_text
                .as_deref(),
            Some("")
        );
    }
    let mut root = trajectory();
    root.steps[0].metadata.as_mut().unwrap().copied = Some(proto::CopiedStep {});
    save(&path, &root);
    assert_eq!(
        conversation(&path).messages.inline()[0]
            .dialogue_text
            .as_deref(),
        Some("")
    );
    for (kind, source) in [(5, 1), (4, 4), (999, 1), (4, 999)] {
        let mut root = trajectory();
        root.kind = kind;
        root.source = source;
        save(&path, &root);
        assert!(
            conversation(&path)
                .messages
                .inline()
                .iter()
                .all(|m| m.dialogue_text.as_deref() == Some(""))
        );
    }
}

#[test]
fn native_file_workspace_metadata_is_unique_and_not_guessed_from_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path());
    let mut root = trajectory();
    let workspace = proto::Workspace {
        project: "file:///work/cbioportal".into(),
        repository: "file:///work/cbioportal".into(),
    };
    root.metadata.as_mut().unwrap().workspaces = vec![workspace.clone(), workspace];
    save(&path, &root);
    let parsed = conversation(&path);
    assert_eq!(parsed.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(parsed.repository, parsed.project);
    root.metadata
        .as_mut()
        .unwrap()
        .workspaces
        .push(proto::Workspace {
            project: "file:///work/other".into(),
            repository: "https://github.com/other/repo".into(),
        });
    save(&path, &root);
    let parsed = conversation(&path);
    assert_eq!(parsed.project, None);
    assert_eq!(parsed.repository, None);
}

#[test]
fn configured_discovery_ignores_archived_markers_temporary_files_and_directories() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path());
    std::fs::write(path.with_extension("pb.archived"), b"").unwrap();
    std::fs::write(dir.path().join("partial.tmp"), b"unfinished").unwrap();
    std::fs::create_dir(dir.path().join("directory.pb")).unwrap();
    let scan = direct_scans(&options(dir.path())).unwrap();
    assert_eq!(scan[0].sources.len(), 1);
    assert_eq!(scan[0].sources[0].identity.path, path);
    assert!(super::super::watch_roots(&options(dir.path())).contains(&dir.path().to_path_buf()));
}

#[test]
fn corrupt_native_snapshot_preserves_search_and_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path());
    let database = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let scans = direct_scans(&options(&path)).unwrap();
    let source = &scans[0].sources[0];
    let stamp = source.stamp().unwrap();
    let original = std::fs::read(&path).unwrap();
    for position in [0, 12, original.len() - 1] {
        let mut corrupt = original.clone();
        corrupt[position] ^= 1;
        std::fs::write(&path, corrupt).unwrap();
        let mut storage = Storage::open_writer(&database).unwrap();
        assert!(index(&mut storage, &options(&path)).is_err());
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 6);
        assert!(
            storage
                .source_checkpoint_matches(Provider::Windsurf, &source.identity, &stamp)
                .unwrap()
        );
        let matches = storage
            .search("cbioportal", 10, Some(Provider::Windsurf), None)
            .unwrap();
        assert_eq!(matches.len(), 3);
        assert_ne!(matches[0].id, matches[1].id);
    }
}

#[test]
fn identity_type_timestamp_truncation_and_prefetch_bounds_fail_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path());
    let mut invalid_roots = Vec::new();
    let mut root = trajectory();
    root.id = "wrong".into();
    invalid_roots.push(root);
    let mut root = trajectory();
    root.steps[0].kind = proto::StepKind::PlannerResponse as i32;
    invalid_roots.push(root);
    let mut root = trajectory();
    root.steps[0].assistant = Some(proto::PlannerResponse::default());
    invalid_roots.push(root);
    for (seconds, nanos) in [(-1, 0), (i64::MAX, 0), (0, -1), (0, 1_000_000_000)] {
        let mut root = trajectory();
        root.metadata.as_mut().unwrap().created_at = Some(proto::Timestamp { seconds, nanos });
        invalid_roots.push(root);
    }
    for root in invalid_roots {
        save(&path, &root);
        assert!(parse(&path).is_err());
    }
    for data in [Vec::new(), vec![0; 27], reader::seal(&[0x12, 0x7f, 0])] {
        std::fs::write(&path, data).unwrap();
        assert!(parse(&path).is_err());
    }
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(8 * 1024 * 1024 + 1).unwrap();
    assert!(parse(&path).err().unwrap().error.message.contains("8 MiB"));
}

#[test]
fn native_repeated_empty_steps_are_rejected_before_container_allocation() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path());
    let bytes = [0x12, 0].repeat(65_537);
    std::fs::write(&path, reader::seal(&bytes)).unwrap();
    assert!(
        parse(&path)
            .err()
            .unwrap()
            .error
            .message
            .contains("field limit")
    );
}
