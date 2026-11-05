use super::*;
use crate::search_results::Results;

fn results(node: Option<&str>) -> Results {
    serde_json::from_value(serde_json::json!([{
        "id":"message", "chat_id":"chat", "provider":"codex", "object":{"role":"entry","kind":"codex:message"},
        "role":"user", "content":"request", "fusion_score":1.0,
        "node_id":node
    }]))
    .unwrap()
}

#[test]
fn references_distinguish_local_combined_shared_only_and_fallback_origins() {
    for (node, available, only, expected) in [
        (None, false, false, None),
        (Some("self"), true, false, None),
        (Some("peer"), true, false, Some("peer")),
        (Some("self"), true, true, Some("self")),
        (Some("self"), false, false, None),
    ] {
        let mut value = results(node);
        attach(&mut value, "self", available, only);
        let Results::Messages(hits) = value else {
            panic!("messages")
        };
        let lookup = hits[0].lookup.as_ref().unwrap();
        assert_eq!(lookup.view.id, "message");
        assert_eq!(lookup.view.shared.as_deref(), expected);
        assert_eq!(lookup.list_attachments.message_id, "message");
        assert_eq!(lookup.list_attachments.shared.as_deref(), expected);
        if expected.is_none() {
            assert!(
                serde_json::to_value(&lookup.view)
                    .unwrap()
                    .get("shared")
                    .is_none()
            );
        }
    }
    let mut value = results(None);
    attach(&mut value, "self", true, true);
    let Results::Messages(hits) = value else {
        panic!("messages")
    };
    assert!(hits[0].lookup.is_none(), "never invent a shared node");
}
