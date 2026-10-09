use super::*;

#[test]
fn search_deserializes_the_shared_core_selection_and_rejects_unknown_fields() {
    let request: Search = serde_json::from_value(json!({
        "query":"database migration", "unit":"messages",
        "provider":["codex", "claude-code"],
        "chat":["thread-a", "thread-b"],
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
        json!({"query":"database", "chat":"thread-a"}),
        json!({"query":"database", "unknown_filter":[]}),
    ] {
        assert!(serde_json::from_value::<Search>(invalid).is_err());
    }
    let empty: Search = serde_json::from_value(json!({"query":"database"})).unwrap();
    assert_eq!(
        empty.filters.match_mode,
        crate::gold::matching::Mode::Flexible
    );
    for (name, expected) in [
        ("strict", crate::gold::matching::Mode::Strict),
        ("prefix", crate::gold::matching::Mode::Prefix),
        ("typo", crate::gold::matching::Mode::Typo),
        ("flexible", crate::gold::matching::Mode::Flexible),
    ] {
        let request: Search =
            serde_json::from_value(json!({"query":"database", "match_mode":name})).unwrap();
        assert_eq!(request.filters.match_mode, expected);
    }
    assert!(
        serde_json::from_value::<Search>(json!({"query":"database", "match_mode":"fuzzy"}))
            .is_err()
    );
    assert!(empty.limit.is_none());
    assert!(empty.unit.is_none());
    assert!(empty.intent.is_none());
    let exact: Search = serde_json::from_value(json!({"query":"voyage-code-4", "intent":"exact-mention", "chat":["thread-a"], "local":true})).unwrap();
    assert_eq!(exact.intent, Some(crate::gold::exact::Intent::ExactMention));
    assert!(
        serde_json::from_value::<Search>(json!({"query":"model", "intent":"decisions"})).is_err()
    );
    assert_eq!(empty.filters.provider, [] as [Provider; 0]);
    assert_eq!(empty.filters.conversation, [] as [String; 0]);
}
