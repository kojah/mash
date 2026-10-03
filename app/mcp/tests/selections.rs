use super::*;

#[test]
fn search_deserializes_the_shared_core_selection_and_rejects_unknown_fields() {
    let request: Search = serde_json::from_value(json!({
        "query":"database migration", "unit":"messages",
        "provider":["codex", "claude-code"],
        "conversation":["thread-a", "thread-b"],
        "after": 2, "before": 3, "local":true
    }))
    .unwrap();
    assert_eq!(
        request.filters.provider,
        [Provider::Codex, Provider::ClaudeCode]
    );
    assert_eq!(request.filters.conversation, ["thread-a", "thread-b"]);
    assert_eq!(request.filters.after, Some(2));
    assert_eq!(request.filters.before, Some(3));
    assert!(matches!(
        request.unit,
        Some(crate::search_results::SearchUnit::Messages)
    ));
    for invalid in [
        json!({"query":"database", "provider":"codex"}),
        json!({"query":"database", "conversation":"thread-a"}),
        json!({"query":"database", "unknown_filter":[]}),
    ] {
        assert!(serde_json::from_value::<Search>(invalid).is_err());
    }
    let empty: Search = serde_json::from_value(json!({"query":"database"})).unwrap();
    assert!(empty.limit.is_none());
    assert!(empty.unit.is_none());
    assert_eq!(empty.filters.provider, [] as [Provider; 0]);
    assert_eq!(empty.filters.conversation, [] as [String; 0]);
}
