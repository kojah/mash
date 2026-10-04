use super::*;
use crate::{role::Role, storage::MessageContent};

fn head() -> NodeHead {
    NodeHead {
        kind: super::super::PointKind::Head,
        node_id: "node".into(),
        version: "version".into(),
        head: None,
        manifest: None,
    }
}
fn request(context: u32, representation: MessageContent) -> protocol::View {
    protocol::View {
        node_id: "node".into(),
        id: "message-5".into(),
        context,
        content: representation,
    }
}
fn message(ordinal: i64, context: MessageContext) -> protocol::MessageRecord {
    protocol::MessageRecord {
        attachments: Vec::new(),
        id: format!("message-{ordinal}"),
        conversation_id: "thread".into(),
        ordinal,
        context,
        role: Role::from("future-role"),
        content: format!("canonical-{ordinal}"),
        projection: format!("projection-{ordinal}"),
        created_at: Some(ordinal),
        generation: None,
        vector: None,
    }
}
fn outline(m: &protocol::MessageRecord) -> api::Record<Outline> {
    api::Record {
        id: point_id(&["message", "version", &m.id]),
        payload: Outline {
            version: "version".into(),
            node_id: "node".into(),
            conversation_id: "thread".into(),
            message: Entry {
                id: m.id.clone(),
                conversation_id: m.conversation_id.clone(),
                ordinal: m.ordinal,
                context: m.context,
            },
        },
    }
}
fn record(m: protocol::MessageRecord) -> api::Record<Document> {
    api::Record {
        id: point_id(&["message", "version", &m.id]),
        payload: Document {
            version: "version".into(),
            node_id: "node".into(),
            provider: crate::provider::Provider::Cursor,
            role: m.role.clone(),
            conversation_id: "thread".into(),
            project: None,
            repository: None,
            created_at: m.created_at,
            tokens: vec![],
            conversation: protocol::ConversationRecord {
                enrichment: None,
                id: "thread".into(),
                provider: crate::provider::Provider::Cursor,
                title: None,
                project: None,
                repository: None,
                created_at: None,
                updated_at: None,
            },
            message: m,
        },
    }
}
fn tree() -> Vec<protocol::MessageRecord> {
    [
        (0, MessageContext::Root),
        (1, MessageContext::Parent { ordinal: 0 }),
        (2, MessageContext::Parent { ordinal: 1 }),
        (3, MessageContext::Root),
        (5, MessageContext::Parent { ordinal: 0 }),
        (6, MessageContext::Parent { ordinal: 5 }),
        (8, MessageContext::Parent { ordinal: 6 }),
    ]
    .into_iter()
    .map(|(n, c)| message(n, c))
    .collect()
}

#[test]
fn projected_topology_preserves_gapped_linear_windows_and_native_branch_paths() {
    for (messages, expected) in [
        (tree(), vec![0, 5, 6, 8]),
        (
            (0..=8)
                .filter(|n| *n != 4)
                .map(|n| message(n, MessageContext::Linear))
                .collect(),
            vec![3, 5, 6, 7],
        ),
    ] {
        let anchor = messages.iter().find(|m| m.ordinal == 5).unwrap();
        let selected = select(
            anchor,
            messages.iter().map(outline).collect(),
            &head(),
            &request(2, MessageContent::Canonical),
        )
        .unwrap();
        assert_eq!(
            selected.entries.keys().copied().collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn topology_rejects_bad_identity_scope_edges_and_duplicate_ordinals() {
    let messages = tree();
    let anchor = &messages[4];
    for case in 0..8 {
        let mut rows: Vec<_> = messages.iter().map(outline).collect();
        match case {
            0 => rows[1].payload.message.context = MessageContext::Parent { ordinal: 5 },
            1 => rows[1].id = "wrong".into(),
            2 => rows[1].payload.node_id = "other".into(),
            3 => rows[1].payload.version = "other".into(),
            4 => rows[1].payload.message.conversation_id = "other".into(),
            5 => rows.push(outline(&messages[1])),
            6 => rows[4].payload.message.context = MessageContext::Root,
            _ => rows.retain(|r| r.payload.message.ordinal != 0),
        }
        assert!(
            select(
                anchor,
                rows,
                &head(),
                &request(2, MessageContent::Canonical)
            )
            .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn selected_record_fetch_rejects_missing_changed_duplicate_or_wrong_scope_records() {
    let messages = tree();
    for case in 0..6 {
        let selected = select(
            &messages[4],
            messages.iter().map(outline).collect(),
            &head(),
            &request(1, MessageContent::Canonical),
        )
        .unwrap();
        let mut records: Vec<_> = [0, 4, 5]
            .into_iter()
            .map(|i| record(messages[i].clone()))
            .collect();
        match case {
            0 => {
                records.pop();
            }
            1 => records[0].payload.node_id = "other".into(),
            2 => records[0].payload.message.context = MessageContext::Linear,
            3 => records[0] = record(messages[4].clone()),
            4 => records[0].payload.version = "other".into(),
            _ => records[0].payload.message.conversation_id = "other".into(),
        }
        assert!(
            selected
                .messages(records, &head(), &request(1, MessageContent::Canonical))
                .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn fetched_context_preserves_opaque_roles_bodies_and_empty_search_projections() {
    let mut messages = tree();
    messages[5].projection.clear();
    for content in [MessageContent::Canonical, MessageContent::SearchProjection] {
        let input = request(1, content);
        let selected = select(
            &messages[4],
            messages.iter().map(outline).collect(),
            &head(),
            &input,
        )
        .unwrap();
        let records = [5, 4, 0]
            .into_iter()
            .map(|i| record(messages[i].clone()))
            .collect();
        let result = selected.messages(records, &head(), &input).unwrap();
        assert_eq!(
            result.iter().map(|m| m.ordinal).collect::<Vec<_>>(),
            [0, 5, 6]
        );
        assert_eq!(result[2].role, Role::from("future-role"));
        assert_eq!(
            result[2].content,
            if matches!(content, MessageContent::Canonical) {
                "canonical-6"
            } else {
                ""
            }
        );
    }
}

#[test]
fn anchor_retrieval_validates_requested_identity_before_traversal() {
    let m = message(5, MessageContext::Root);
    let input = request(0, MessageContent::Canonical);
    assert!(anchor(vec![], &head(), &input).unwrap().is_none());
    assert!(
        anchor(vec![record(m.clone())], &head(), &input)
            .unwrap()
            .is_some()
    );
    assert!(anchor(vec![record(m.clone()), record(m.clone())], &head(), &input).is_err());
    let mut wrong = record(m);
    wrong.payload.message.id = "other".into();
    assert!(anchor(vec![wrong], &head(), &input).is_err());
}
