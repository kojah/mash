use super::*;
use crate::views::config::{DenseContent, EmbeddingText, TextPreparation};

fn text(
    stream: &mut Stream,
    build: &Build,
    chat: &str,
    ordinal: i64,
    context: &MessageContext,
    role: &Role,
    projection: &str,
) -> Option<String> {
    stream
        .prepare(
            build,
            Input {
                chat,
                id: &ordinal.to_string(),
                ordinal,
                context,
                role,
                projection,
            },
            None,
        )
        .unwrap()
}

#[test]
fn parent_units_respect_branches_roots_gaps_visibility_and_role_selection() {
    let build = Build {
        retrieval_unit: RetrievalUnit::ParentAndMessage,
        ..Default::default()
    };
    let mut stream = Stream::default();
    assert_eq!(
        text(
            &mut stream,
            &build,
            "a",
            0,
            &MessageContext::Root,
            &Role::User,
            "root"
        ),
        Some("root".into())
    );
    assert_eq!(
        text(
            &mut stream,
            &build,
            "a",
            1,
            &MessageContext::Linear,
            &Role::Assistant,
            "answer"
        ),
        Some("answer\n\nParent message:\nroot".into())
    );
    assert_eq!(
        text(
            &mut stream,
            &build,
            "a",
            2,
            &MessageContext::Parent { id: "0".into() },
            &Role::User,
            "branch"
        ),
        Some("branch\n\nParent message:\nroot".into())
    );
    assert_eq!(
        text(
            &mut stream,
            &build,
            "a",
            3,
            &MessageContext::Linear,
            &Role::User,
            ""
        ),
        None
    );
    assert_eq!(
        text(
            &mut stream,
            &build,
            "a",
            4,
            &MessageContext::Linear,
            &Role::User,
            "after hidden"
        ),
        Some("after hidden".into())
    );
}

#[test]
fn parent_units_respect_gaps_roots_thread_boundaries_and_role_selection() {
    let build = Build {
        retrieval_unit: RetrievalUnit::ParentAndMessage,
        ..Default::default()
    };
    let mut stream = Stream::default();
    text(
        &mut stream,
        &build,
        "a",
        4,
        &MessageContext::Root,
        &Role::User,
        "previous",
    );
    assert_eq!(
        text(
            &mut stream,
            &build,
            "a",
            6,
            &MessageContext::Linear,
            &Role::User,
            "gap"
        ),
        Some("gap".into())
    );
    assert_eq!(
        text(
            &mut stream,
            &build,
            "a",
            7,
            &MessageContext::Root,
            &Role::User,
            "new root"
        ),
        Some("new root".into())
    );
    assert_eq!(
        text(
            &mut stream,
            &build,
            "b",
            1,
            &MessageContext::Parent { id: "0".into() },
            &Role::User,
            "other chat"
        ),
        Some("other chat".into())
    );
    let users = Build {
        dense_content: DenseContent::UserMessages,
        ..build
    };
    let mut stream = Stream::default();
    assert_eq!(
        text(
            &mut stream,
            &users,
            "a",
            0,
            &MessageContext::Root,
            &Role::Assistant,
            "excluded"
        ),
        None
    );
    assert_eq!(
        text(
            &mut stream,
            &users,
            "a",
            1,
            &MessageContext::Linear,
            &Role::User,
            "user"
        ),
        Some("user".into())
    );
}

#[test]
fn unit_contract_is_bounded_unicode_safe_versioned_and_strict() {
    let default = Build::default();
    assert!(
        !serde_json::to_string(&default)
            .unwrap()
            .contains("retrieval_unit")
    );
    let build: Build = toml::from_str("retrieval_unit='parent-and-message'").unwrap();
    assert_ne!(default, build);
    assert!(toml::from_str::<Build>("retrieval_unit='chat'").is_err());
    assert!(toml::from_str::<Build>("retrieval_unit=true").is_err());
    assert!(
        toml::from_str::<super::super::config::Configuration>(
            "[query]\nretrieval_unit='parent-and-message'"
        )
        .is_err()
    );
    let parent = "é".repeat(PARENT_CHARS + 1);
    assert_eq!(
        build.prepare_unit("anchor".into(), None, Some(&parent)),
        format!("anchor\n\nParent message:\n{}", "é".repeat(PARENT_CHARS))
    );
    assert_eq!(
        build.prepare_unit(String::new(), Some("title"), Some(&parent)),
        ""
    );
    assert_eq!(
        default.prepare_unit("anchor".into(), None, Some(&parent)),
        "anchor"
    );
    let limited = Build {
        max_embedding_chars: Some(256.try_into().unwrap()),
        embedding_text: EmbeddingText::TitleAndDialogue,
        text_preparation: TextPreparation::CollapseWhitespace,
        ..build
    };
    let prepared = limited.prepare_unit("é".repeat(300), Some("title"), Some("parent"));
    assert_eq!(prepared.chars().count(), 256);
    assert!(!prepared.contains("parent"));
}

#[test]
fn invalid_stream_order_and_edges_fail_without_default_scratch_overhead() {
    let mut stream = Stream::default();
    text(
        &mut stream,
        &Build::default(),
        "a",
        0,
        &MessageContext::Root,
        &Role::User,
        "a",
    );
    assert!(stream.parents.is_none());
    let build = Build {
        retrieval_unit: RetrievalUnit::ParentAndMessage,
        ..Default::default()
    };
    text(
        &mut stream,
        &build,
        "a",
        1,
        &MessageContext::Root,
        &Role::User,
        "a",
    );
    for (ordinal, context) in [
        (1, MessageContext::Linear),
        (0, MessageContext::Root),
        (2, MessageContext::Parent { id: "2".into() }),
    ] {
        assert!(
            stream
                .prepare(
                    &build,
                    Input {
                        chat: "a",
                        id: &ordinal.to_string(),
                        ordinal,
                        context: &context,
                        role: &Role::User,
                        projection: "bad"
                    },
                    None
                )
                .is_err()
        );
    }
}
