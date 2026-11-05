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
        json!({"query":"database", "source":["external"]}),
        json!({"query":"database", "kind":["codex:message"]}),
        json!({"query":"database", "filter":{"source":["external"]}}),
        json!({"query":"database", "filter":{"kind":["codex:message"]}}),
    ] {
        assert!(serde_json::from_value::<Search>(invalid).is_err());
    }
    let empty: Search = serde_json::from_value(json!({"query":"database"})).unwrap();
    assert_eq!(
        empty.filters.match_mode,
        crate::views::matching::Mode::Flexible
    );
    for (name, expected) in [
        ("strict", crate::views::matching::Mode::Strict),
        ("prefix", crate::views::matching::Mode::Prefix),
        ("typo", crate::views::matching::Mode::Typo),
        ("flexible", crate::views::matching::Mode::Flexible),
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
    assert_eq!(
        exact.intent,
        Some(crate::views::exact::Intent::ExactMention)
    );
    assert!(
        serde_json::from_value::<Search>(json!({"query":"model", "intent":"decisions"})).is_err()
    );
    assert_eq!(empty.filters.provider, [] as [Provider; 0]);
    assert_eq!(empty.filters.conversation, [] as [String; 0]);
}
