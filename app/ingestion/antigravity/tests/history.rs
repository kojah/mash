use super::*;

#[test]
fn absent_display_text_uses_ordered_text_and_keeps_scope_only_content_unsearchable() {
    let root = store::Root {
        trajectory: "trajectory".into(),
        cascade: "cascade".into(),
        kind: 4,
        source: 1,
    };
    let metadata = proto::Metadata::default();
    let mut step = proto::Step {
        kind: proto::Kind::User as i32,
        status: proto::Status::Done as i32,
        metadata: Some(proto::StepMetadata {
            source: proto::Source::UserExplicit as i32,
            ..Default::default()
        }),
        user: Some(proto::User {
            items: vec![
                proto::Item {
                    text: "Explain ".into(),
                    ..Default::default()
                },
                proto::Item {
                    text: "cBioPortal".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        super::super::history::normalize(&root, &metadata, 0, &step)
            .unwrap()
            .unwrap()
            .search_projection
            .as_deref(),
        Some("Explain cBioPortal")
    );
    let user = step.user.as_mut().unwrap();
    user.items.clear();
    user.legacy_query = "Legacy cBioPortal query".into();
    assert_eq!(
        super::super::history::normalize(&root, &metadata, 0, &step)
            .unwrap()
            .unwrap()
            .content,
        "Legacy cBioPortal query"
    );
    let user = step.user.as_mut().unwrap();
    user.legacy_query.clear();
    user.items.push(proto::Item {
        text: String::new(),
        scope: Some(proto::Scope {
            text_block: Some(proto::TextBlock {
                label: "Context".into(),
                content: "scope marker".into(),
            }),
        }),
    });
    assert_eq!(
        super::super::history::normalize(&root, &metadata, 0, &step)
            .unwrap()
            .unwrap()
            .search_projection
            .as_deref(),
        Some("")
    );
    let reasoning = proto::Step {
        kind: proto::Kind::Assistant as i32,
        status: proto::Status::Done as i32,
        metadata: Some(proto::StepMetadata {
            source: proto::Source::Model as i32,
            ..Default::default()
        }),
        assistant: Some(proto::Assistant {
            thinking: "internal analysis".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let message = super::super::history::normalize(&root, &metadata, 1, &reasoning)
        .unwrap()
        .unwrap();
    assert_eq!(message.content, "internal analysis");
    assert_eq!(message.search_projection.as_deref(), Some(""));
}

#[test]
fn native_dialogue_keeps_identities_and_separates_scope_and_reasoning() {
    let directory = tempfile::tempdir().unwrap();
    let path = write(directory.path());
    let parsed = conversation(&path);
    assert_eq!(parsed.messages.len(), 9);
    assert_eq!(parsed.messages[0].content, parsed.messages[1].content);
    assert_ne!(parsed.messages[0].id, parsed.messages[1].id);
    assert!(
        parsed
            .messages
            .iter()
            .all(|m| !m.search_projection.as_deref().unwrap().is_empty())
    );
    for message in parsed.messages.iter().filter(|m| m.role == Role::Assistant) {
        assert!(message.content.contains("Synthetic reasoning"));
        assert!(
            !message
                .search_projection
                .as_deref()
                .unwrap()
                .contains("Synthetic reasoning")
        );
        assert_eq!(message.created_at, None);
    }
    let scoped = parsed.messages.last().unwrap();
    assert!(scoped.content.contains("synthetic_scope_only_marker"));
    assert!(
        !scoped
            .search_projection
            .as_deref()
            .unwrap()
            .contains("synthetic_scope_only_marker")
    );
    assert_eq!(
        scoped.search_projection.as_deref(),
        Some("Explain cBioPortal @[Attached context] mutation filtering.")
    );
    assert!(parsed.created_at.is_some());
    assert_eq!(parsed.provider, Provider::Antigravity);
}
#[test]
fn native_fork_copies_remain_canonical_but_not_searchable() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_case(directory.path(), &fixture().fork);
    let parsed = conversation(&path);
    assert_eq!(parsed.messages.len(), 2);
    assert!(
        parsed
            .messages
            .iter()
            .all(|m| m.search_projection.as_deref() == Some(""))
    );
}
#[test]
fn unknown_origins_subagents_and_invalid_statuses_never_enter_search() {
    let directory = tempfile::tempdir().unwrap();
    let path = write(directory.path());
    let original = proto::Step::decode(
        fixture().root_before_mutation.dialogue_steps[0]
            .step_payload
            .bytes()
            .as_slice(),
    )
    .unwrap();
    for source in [0, 1, 2, 3, 5, 6, 999] {
        let mut step = original.clone();
        step.metadata.as_mut().unwrap().source = source;
        replace_step(&path, 0, &step);
        assert_eq!(
            conversation(&path).messages[0].search_projection.as_deref(),
            Some("")
        );
    }
    for status in [0, 1, 2, 5, 7, 999] {
        let mut step = original.clone();
        step.status = status;
        replace_step(&path, 0, &step);
        assert_eq!(
            conversation(&path).messages[0].search_projection.as_deref(),
            Some("")
        );
    }
    let mut missing = original.clone();
    missing.user = None;
    replace_step(&path, 0, &missing);
    assert!(parse(&source(&path)).is_err());
    replace_step(&path, 0, &original);
    let writer = Connection::open(&path).unwrap();
    let bytes: Vec<u8> = writer
        .query_row(
            "SELECT data FROM trajectory_metadata_blob WHERE id='main'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut metadata = proto::Metadata::decode(bytes.as_slice()).unwrap();
    metadata.subagent = Some(proto::Subagent {});
    writer
        .execute(
            "UPDATE trajectory_metadata_blob SET data=?1 WHERE id='main'",
            [metadata.encode_to_vec()],
        )
        .unwrap();
    assert!(
        conversation(&path)
            .messages
            .iter()
            .all(|m| m.search_projection.as_deref() == Some(""))
    );
}
#[test]
fn metadata_locations_are_unambiguous_and_bad_timestamps_fail() {
    let directory = tempfile::tempdir().unwrap();
    let path = write(directory.path());
    let writer = Connection::open(&path).unwrap();
    let bytes: Vec<u8> = writer
        .query_row(
            "SELECT data FROM trajectory_metadata_blob WHERE id='main'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut metadata = proto::Metadata::decode(bytes.as_slice()).unwrap();
    metadata.workspaces = vec![proto::Workspace {
        project: "file:///tmp/project".into(),
        repository: "file:///tmp/repo".into(),
    }];
    writer
        .execute(
            "UPDATE trajectory_metadata_blob SET data=?1 WHERE id='main'",
            [metadata.encode_to_vec()],
        )
        .unwrap();
    let parsed = conversation(&path);
    assert_eq!(parsed.project.as_deref(), Some("/tmp/project"));
    assert_eq!(parsed.repository.as_deref(), Some("/tmp/repo"));
    metadata.workspaces.push(proto::Workspace {
        project: "file:///tmp/other".into(),
        repository: "https://example.com/repo".into(),
    });
    writer
        .execute(
            "UPDATE trajectory_metadata_blob SET data=?1 WHERE id='main'",
            [metadata.encode_to_vec()],
        )
        .unwrap();
    let parsed = conversation(&path);
    assert_eq!(parsed.project, None);
    assert_eq!(parsed.repository, None);
    metadata.created_at.as_mut().unwrap().nanos = 1_000_000_000;
    writer
        .execute(
            "UPDATE trajectory_metadata_blob SET data=?1 WHERE id='main'",
            [metadata.encode_to_vec()],
        )
        .unwrap();
    assert!(parse(&source(&path)).is_err());
}
