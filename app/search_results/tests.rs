use super::*;

#[test]
fn message_replica_dedup_preserves_repeated_and_divergent_canonical_messages() {
    let original = hit("thread", "message");
    let mut replica = hit("thread", "message");
    replica.node_id = Some("peer".into());
    let mut repeated = hit("thread", "repeated-message");
    repeated.content.clone_from(&original.content);
    let mut divergent = hit("thread", "message");
    divergent.node_id = Some("peer-diverged".into());
    divergent.content = "changed history".into();
    let mut another_provider = hit("thread", "message");
    another_provider.provider = Provider::ClaudeCode;
    let another_thread = hit("other-thread", "message");
    let rows = deduplicate(
        vec![
            original,
            replica,
            repeated,
            divergent,
            another_provider,
            another_thread,
        ],
        SearchUnit::Messages,
    );
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].copies.len(), 1);
    assert_eq!(rows[0].copies[0].node_id.as_deref(), Some("peer"));
    assert_eq!(rows[1].id, "repeated-message");
    assert_eq!(rows[2].content, "changed history");
    assert_eq!(rows[3].provider, Provider::ClaudeCode);
    assert_eq!(rows[4].conversation_id, "other-thread");
}

#[test]
fn message_replicas_fuse_once_even_when_rankers_choose_different_nodes() {
    let mut lexical_copy = hit("thread", "a");
    lexical_copy.node_id = Some("peer".into());
    let mut semantic_copy = hit("thread", "a");
    semantic_copy.node_id = Some("other-peer".into());
    semantic_copy.semantic_score = Some(0.9);
    let rows = crate::semantic::fuse(
        vec![hit("thread", "a"), lexical_copy, hit("thread", "b")],
        vec![semantic_copy, hit("thread", "b")],
        SearchUnit::Messages,
        crate::gold::config::Fusion::default(),
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "a");
    assert_eq!(rows[0].copies.len(), 2);
    assert_eq!(rows[0].semantic_score, Some(0.9));
    assert!((rows[0].fusion_score - 2.0 / 61.0).abs() < 1e-6);
    assert!((rows[1].fusion_score - 2.0 / 62.0).abs() < 1e-6);
}

#[test]
fn weak_semantic_neighbours_abstain_but_negative_logit_paraphrases_survive() {
    for unit in [SearchUnit::Threads, SearchUnit::Messages] {
        let mut useful = hit("useful", "model caching");
        useful.lexical_score = None;
        useful.semantic_score = Some(0.396);
        useful.rerank_score = Some(-1.704);
        let mut weak = hit("weak", "unrelated message");
        weak.lexical_score = None;
        weak.semantic_score = Some(0.253);
        weak.rerank_score = Some(-2.958);
        let results = collect(vec![weak, useful], unit, 5, "keep the model warm");
        match results {
            Results::Threads(hits) => assert_eq!(
                hits.iter()
                    .map(|h| h.conversation_id.as_str())
                    .collect::<Vec<_>>(),
                ["useful"]
            ),
            Results::Messages(hits) => assert_eq!(
                hits.iter()
                    .map(|h| h.conversation_id.as_str())
                    .collect::<Vec<_>>(),
                ["useful"]
            ),
        }
    }
}

fn hit(thread: &str, id: &str) -> SearchHit {
    SearchHit {
        id: id.into(),
        conversation_id: thread.into(),
        provider: Provider::Codex,
        role: crate::role::Role::User,
        content: id.into(),
        copies: Vec::new(),
        created_at: Some(123),
        metadata: ThreadMetadata {
            enrichment: None,
            title: Some(thread.into()),
            ..Default::default()
        },
        lexical_score: Some(1.0),
        lookup: None,
        lexical_match: None,
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
fn explicit_references_cannot_be_replaced_by_nearby_issue_numbers() {
    for unit in [SearchUnit::Threads, SearchUnit::Messages] {
        let mut wrong = hit("wrong", "wrong-pr");
        wrong.content = "review PR 183".into();
        wrong.rerank_score = Some(3.0);
        let mut correct = hit("correct", "correct-pr");
        correct.content = "https://github.com/cBioPortal/cbioportal-core/pull/180".into();
        // Exact references remain evidence outside the reranking/FTS budget.
        correct.lexical_score = None;
        correct.rerank_score = None;
        match collect(vec![wrong, correct], unit, 5, "PR 180") {
            Results::Threads(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].conversation_id, "correct");
            }
            Results::Messages(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, "correct-pr");
            }
        }
    }
}

#[test]
fn strongest_evidence_dominates_corroboration_and_excerpts_are_capped() {
    let hits = vec![
        hit("single", "best"),
        hit("multiple", "second"),
        hit("multiple", "third"),
        hit("multiple", "fourth"),
        hit("multiple", "fifth"),
    ];
    let results = threads(hits, 2);
    assert_eq!(results[0].conversation_id, "single");
    assert_eq!(results[1].conversation_id, "multiple");
    assert_eq!(
        results[1]
            .matches
            .iter()
            .map(|m| m.message_id.as_str())
            .collect::<Vec<_>>(),
        ["second", "third", "fourth"]
    );
    assert!((results[1].score - (1.0 / 62.0 + (1.0 / 62.0 - 1.0 / 63.0) * 0.5)).abs() < 0.000_001);
    assert_eq!(results[1].metadata.title.as_deref(), Some("multiple"));
}

#[test]
fn retrieval_neighbours_are_not_automatically_thread_evidence() {
    let mut relevant = hit("skills", "skills-transfer");
    relevant.content = "Are the cbioportal-related skills transferred?".into();
    relevant.rerank_score = Some(0.89);
    let mut greeting = hit("skills", "greeting");
    greeting.content = "hello!".into();
    greeting.lexical_score = None;
    let mut filler = hit("skills", "filler");
    filler.content = "sounds good keep going".into();
    filler.lexical_score = None;
    let mut unrelated = hit("irrelevant", "nearest-neighbour");
    unrelated.lexical_score = None;
    unrelated.semantic_score = Some(0.8);
    let results = threads(vec![relevant, greeting, filler, unrelated], 5);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].matches.len(), 1);
    assert_eq!(results[0].matches[0].message_id, "skills-transfer");
    assert_eq!(results[0].score, 1.0 / 61.0);
}

#[test]
fn semantic_paraphrases_and_lexical_tail_survive_without_forcing_a_full_page() {
    let mut paraphrase = hit("semantic", "meaningful-follow-up");
    paraphrase.lexical_score = None;
    paraphrase.rerank_score = Some(0.3);
    paraphrase.content = "What are the comments you'd make?".into();
    let mut lexical = hit("lexical", "outside-reranking-budget");
    lexical.rerank_score = Some(-1.0);
    let mut unknown = hit("noise", "unscored");
    unknown.lexical_score = None;
    let results = threads(vec![paraphrase, lexical, unknown], 20);
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0].matches[0].snippet,
        "What are the comments you'd make?"
    );
    for score in [None, Some(f32::NAN), Some(f32::INFINITY)] {
        let mut unknown = hit("noise", "unscored");
        unknown.lexical_score = None;
        unknown.rerank_score = score;
        assert!(threads(vec![unknown], 20).is_empty());
    }
    // A negative uncalibrated logit is not grounds to drop a semantic paraphrase.
    let mut paraphrase = hit("semantic", "login-token");
    paraphrase.lexical_score = None;
    paraphrase.rerank_score = Some(-0.3);
    paraphrase.semantic_score = Some(0.5);
    assert_eq!(threads(vec![paraphrase], 20).len(), 1);
    let mut unknown = hit("noise", "unscored");
    unknown.lexical_score = None;
    let Results::Messages(results) = collect(vec![unknown], SearchUnit::Messages, 20, "query")
    else {
        panic!("messages");
    };
    assert!(results.is_empty());
}

#[test]
fn reranking_windows_reveal_late_evidence_without_changing_short_messages() {
    let short = "what are your comments on this?";
    assert!(matches!(
        rerank_document(short, "cbioportal"),
        std::borrow::Cow::Borrowed(_)
    ));
    let long = format!(
        "{}cBioPortal importer {}",
        "unrelated introduction ".repeat(500),
        "🦀".repeat(3000)
    );
    let window = rerank_document(&long, "cbioportal importer");
    assert!(window.contains("cBioPortal importer"));
    assert!(window.chars().count() <= 2402);
    let short_window = evidence_window(&long, "cbioportal importer", 160);
    assert!(short_window.contains("cBioPortal importer"));
    assert!(short_window.chars().count() <= 162);
    assert!(window.starts_with('…'));
    assert!(rerank_document(&long, "semantic paraphrase").starts_with("unrelated introduction"));
    let natural_query = format!(
        "{}cBioPortal importer",
        "the introduction and context ".repeat(500)
    );
    assert!(
        rerank_document(&natural_query, "what about the cBioPortal importer?")
            .contains("cBioPortal importer")
    );
}

#[test]
fn contextual_evidence_uses_nearest_searchable_messages_with_a_fixed_budget() {
    let anchor = hit("thread", "anchor");
    let message = |id: &str, role, content: String| crate::storage::Message {
        attachments: Vec::new(),
        id: id.into(),
        ordinal: 0,
        role,
        content,
        created_at: None,
    };
    let messages = vec![
        message(
            "distant",
            crate::role::Role::User,
            "distant unrelated text".into(),
        ),
        message(
            "before",
            crate::role::Role::User,
            "cache earlier ".repeat(100),
        ),
        message("tool", crate::role::Role::Tool, "tool noise".into()),
        message("anchor", crate::role::Role::User, anchor.content.clone()),
        message(
            "after",
            crate::role::Role::Assistant,
            "cache later ".repeat(100),
        ),
    ];
    let document = contextual_document(&anchor, "cache", &messages);
    assert!(document.contains("cache earlier"));
    assert!(document.contains("cache later"));
    assert!(!document.contains("distant unrelated"));
    assert!(!document.contains("tool noise"));
    assert!(document.chars().count() < 800);
    assert_eq!(
        contextual_document(&anchor, "cache", &[]),
        "Matched message: anchor"
    );
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
        results[0].matches[0].copies,
        vec![MessageReference {
            message_id: "remote".into(),
            node_id: Some("remote-node".into())
        }]
    );
    assert_eq!(
        results[0].matches[1].node_id.as_deref(),
        Some("remote-node")
    );
}

#[test]
fn replicas_and_context_do_not_shift_other_threads_or_hide_real_requests() {
    let first = hit("a", "first");
    let second = hit("b", "second");
    let baseline = threads(vec![hit("a", "first"), hit("b", "second")], 20);
    let mut copy = hit("a", "first");
    copy.id = "copy".into();
    copy.node_id = Some("replica".into());
    copy.content = "<system-reminder>noise</system-reminder>first".into();
    let mut excluded = hit("context", "injection");
    excluded.content = "Base directory for this skill: /skills/cbioportal\nnoise".into();
    let actual = threads(vec![excluded, first, copy, second], 20);
    assert_eq!(actual.len(), baseline.len());
    for (actual, expected) in actual.iter().zip(baseline) {
        assert_eq!(actual.conversation_id, expected.conversation_id);
        assert_eq!(actual.score, expected.score);
    }
    assert_eq!(actual[0].matches[0].copies.len(), 1);
    assert_eq!(actual[0].matches[0].snippet, "first");
    // Independently asked identical questions remain distinct conversations.
    let mut independent = hit("independent", "new-message");
    independent.content = "first".into();
    assert_eq!(threads(vec![hit("a", "first"), independent], 20).len(), 2);
}

#[test]
fn message_results_apply_exclusions_without_changing_the_view_anchor() {
    let mut mixed = hit("a", "request");
    mixed.content = "<environment_context>noise</environment_context>actual question".into();
    let mut system = hit("a", "system");
    system.role = crate::role::Role::System;
    let Results::Messages(results) =
        collect(vec![system, mixed], SearchUnit::Messages, 20, "question")
    else {
        panic!("messages")
    };
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "request");
    assert_eq!(results[0].content, "actual question");
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
        [
            "message-0",
            "short-match",
            "message-1",
            "message-2",
            "message-3",
            "message-4",
            "message-5"
        ]
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
fn exact_excerpts_show_verified_literals_instead_of_earlier_partial_decoys() {
    for (query, decoy, matched) in [
        ("if", "gift", "IF"),
        ("\"foo foo\"", "xfoo foo", "foo foo"),
        ("voyage-code-4", "voyage-code-40", "VOYAGE-code-4"),
        (
            "\"database timeout\"",
            "database health",
            "database timeout",
        ),
        ("C++", "C++20", "c++"),
    ] {
        let literal = crate::gold::exact::Literal::new(query).unwrap();
        let mut evidence = hit("thread", "message");
        evidence.content = format!(
            "{decoy} {} ({matched}) {}",
            "🦀 ".repeat(1200),
            "界".repeat(1200)
        );
        evidence.lexical_score = None;
        let projection = evidence.content.clone();
        let mut wrong = hit("wrong", "wrong");
        wrong.content = decoy.into();
        let Results::Threads(results) =
            collect_exact(vec![wrong, evidence], SearchUnit::Threads, 2, &literal)
        else {
            panic!("threads");
        };
        assert_eq!(results.len(), 1, "{query}");
        let anchor = &results[0].matches[0];
        assert_eq!(anchor.message_id, "message");
        assert!(anchor.snippet.contains(matched), "{query}");
        assert!(!anchor.snippet.contains(decoy), "{query}");
        assert!(anchor.snippet.starts_with('…') && anchor.snippet.ends_with('…'));
        assert!(anchor.snippet.chars().count() <= 802);
        assert_projection_span(&projection, &anchor.snippet, &anchor.projection_span);
    }
}

#[test]
fn excerpts_are_bounded_unicode_safe_and_focus_on_query_evidence() {
    let content = format!(
        "{}cBioPortal importer {}",
        "🦀".repeat(1200),
        "界".repeat(1200)
    );
    let (result, span) = evidence_excerpt(&content, "cbioportal", 800);
    assert!(result.contains("cBioPortal importer"));
    assert!(result.starts_with('…') && result.ends_with('…'));
    assert!(result.chars().count() <= 802);
    assert_projection_span(&content, &result, &span);
}

fn assert_projection_span(projection: &str, snippet: &str, span: &ProjectionSpan) {
    let characters: Vec<_> = projection.chars().collect();
    assert!(span.start_char <= span.end_char && span.end_char <= characters.len());
    let excerpt: String = characters[span.start_char..span.end_char].iter().collect();
    let rendered = format!(
        "{}{}{}",
        if span.start_char > 0 { "…" } else { "" },
        excerpt,
        if span.end_char < characters.len() {
            "…"
        } else {
            ""
        }
    );
    assert_eq!(snippet, rendered);
}

#[test]
fn projection_spans_cover_empty_short_edge_and_combining_character_windows() {
    for (text, query) in [
        (String::new(), "missing"),
        ("…cafe\u{301} 🦀…".into(), "missing"),
        (format!("{}target", "界".repeat(1000)), "target"),
        (format!("target{}", "🦀".repeat(1000)), "target"),
    ] {
        let (snippet, span) = evidence_excerpt(&text, query, 800);
        assert_projection_span(&text, &snippet, &span);
        assert!(span.end_char - span.start_char <= 800);
    }
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
    assert_eq!(
        decoded[0].matches[0].projection_span,
        ProjectionSpan {
            start_char: 0,
            end_char: decoded[0].matches[0].snippet.chars().count()
        }
    );
}
