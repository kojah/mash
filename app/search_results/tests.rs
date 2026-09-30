use super::*;

fn hit(thread: &str, id: &str) -> SearchHit {
    SearchHit {
        id: id.into(),
        conversation_id: thread.into(),
        provider: Provider::Codex,
        role: crate::role::Role::User,
        content: id.into(),
        created_at: Some(123),
        metadata: ThreadMetadata {
            title: Some(thread.into()),
            ..Default::default()
        },
        lexical_score: None,
        semantic_score: None,
        fusion_score: 0.03,
        rerank_score: None,
        node_id: None,
    }
}

fn threads(hits: Vec<SearchHit>, limit: usize) -> Vec<ThreadHit> {
    let Results::Threads(results) = collect(hits, SearchUnit::Threads, limit, "query") else {
        panic!("threads");
    };
    results
}

#[test]
fn corroboration_is_discounted_and_capped_before_the_thread_limit() {
    let hits = vec![
        hit("single", "best"),
        hit("multiple", "second"),
        hit("multiple", "third"),
        hit("multiple", "fourth"),
        hit("multiple", "fifth"),
    ];
    let results = threads(hits, 1);
    assert_eq!(results[0].conversation_id, "multiple");
    assert_eq!(
        results[0]
            .matches
            .iter()
            .map(|m| m.message_id.as_str())
            .collect::<Vec<_>>(),
        ["second", "third", "fourth"]
    );
    assert!((results[0].score - (1.0 / 62.0 + 0.1 / 63.0 + 0.05 / 64.0)).abs() < 0.000_001);
    assert_eq!(results[0].metadata.title.as_deref(), Some("multiple"));
}

#[test]
fn repeated_content_and_cross_node_copies_do_not_boost_a_thread() {
    let mut first = hit("same", "local");
    first.content = "same message".into();
    let mut copy = hit("same", "remote");
    copy.node_id = Some("remote-node".into());
    copy.content = " same\n  message ".into();
    let mut next = hit("same", "next");
    next.node_id = Some("remote-node".into());
    let results = threads(diversify(vec![first, copy, next], SearchUnit::Threads), 20);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].matches.len(), 2);
    assert_eq!(results[0].matches[0].node_id, None);
    assert_eq!(
        results[0].matches[1].node_id.as_deref(),
        Some("remote-node")
    );
}

#[test]
fn candidate_diversity_prevents_one_chat_from_occupying_the_rerank_budget() {
    let mut hits = (0..40)
        .map(|i| hit("long", &format!("message-{i}")))
        .collect::<Vec<_>>();
    hits.push(hit("short", "short-match"));
    let hits = diversify(hits, SearchUnit::Threads);
    assert_eq!(
        hits.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
        ["message-0", "short-match", "message-1", "message-2"]
    );
}

#[test]
fn provider_identity_limits_and_message_mode_are_preserved() {
    let first = hit("same-id", "first");
    let mut second = hit("same-id", "second");
    second.provider = Provider::ClaudeCode;
    assert_eq!(threads(vec![first, second], 20).len(), 2);
    assert!(threads(vec![hit("a", "a")], 0).is_empty());
    let Results::Messages(messages) = collect(
        diversify(vec![hit("a", "1"), hit("a", "2")], SearchUnit::Messages),
        SearchUnit::Messages,
        1,
        "query",
    ) else {
        panic!("messages");
    };
    assert_eq!(messages[0].id, "1");
}

#[test]
fn excerpts_are_bounded_unicode_safe_and_focus_on_query_evidence() {
    let content = format!(
        "{}cBioPortal importer {}",
        "🦀".repeat(1200),
        "界".repeat(1200)
    );
    let result = excerpt(&content, "cbioportal");
    assert!(result.contains("cBioPortal importer"));
    assert!(result.starts_with('…') && result.ends_with('…'));
    assert!(result.chars().count() <= 802);
}

#[test]
fn thread_metadata_and_view_anchors_survive_worker_serialization() {
    let results = collect(
        vec![hit("chat", "message")],
        SearchUnit::Threads,
        20,
        "query",
    );
    let json = serde_json::to_string(&results).unwrap();
    let Results::Threads(decoded) = serde_json::from_str(&json).unwrap() else {
        panic!("threads");
    };
    assert_eq!(decoded[0].metadata.title.as_deref(), Some("chat"));
    assert_eq!(decoded[0].matches[0].message_id, "message");
    assert_eq!(decoded[0].matches[0].created_at, Some(123));
}
