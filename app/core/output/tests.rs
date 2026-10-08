use super::*;
use serde_json::json;

#[test]
fn compact_messages_preserve_late_unicode_evidence_anchors_and_errors() {
    let content = format!("{}needle {}", "界 ".repeat(900), "終 ".repeat(600));
    let response = Response::Search(serde_json::from_value(json!({
        "query":"needle", "intent":"exact-mention", "realized_mode":"merged-lexical",
        "unit":"messages", "shared":true, "shared_error":"peer unavailable",
        "shared_context_error":"context unavailable",
        "results":[{
            "id":"primary", "conversation_id":"thread", "provider":"codex", "role":"user",
            "node_id":"peer", "content":content, "fusion_score":0.3, "rerank_score":0.8,
            "copies":[{"message_id":"copy","node_id":"other-peer"}],
            "metadata":{"title":"title","enrichment":{"version":1,"title":null,"project":null,"repository":null}}
        }]
    })).unwrap());
    let full = serialize(&response, Detail::Extended).unwrap();
    let compact = serialize(&response, Detail::Compact).unwrap();
    assert!(compact.to_string().len() < full.to_string().len() / 2);
    let hit = &compact["results"][0];
    assert!(hit["content"].as_str().unwrap().contains("needle"));
    assert!(hit["content"].as_str().unwrap().chars().count() <= EXCERPT_CHARS + 2);
    assert_eq!(hit["content_truncated"], true);
    for key in [
        "id",
        "conversation_id",
        "provider",
        "role",
        "node_id",
        "copies",
    ] {
        assert_eq!(hit[key], full["results"][0][key]);
    }
    for key in [
        "shared",
        "shared_error",
        "shared_context_error",
        "unit",
        "intent",
    ] {
        assert_eq!(compact[key], full[key]);
    }
    assert!(hit.get("rerank_score").is_none());
    assert!(hit["metadata"].get("enrichment").is_none());
    assert_eq!(full["results"][0]["content"], content);
    assert_eq!(full, serialize(&response, Detail::Extended).unwrap());
}

#[test]
fn compact_thread_excerpts_preserve_copy_anchors_and_order() {
    let response = Response::Search(
        serde_json::from_value(json!({
            "query":"needle", "realized_mode":"lexical", "unit":"threads", "shared":true,
            "results":[{"conversation_id":"thread","provider":"codex","node_id":"peer","score":0.5,
            "matches":[{"message_id":"message","node_id":"peer","role":"assistant","created_at":1,
                "snippet":"x".repeat(800),"projection_span":{"start_char":50,"end_char":850},
                "rerank_score":0.9,"fusion_score":0.5,
                "copies":[{"message_id":"copy","node_id":"other"}]}]}]
        }))
        .unwrap(),
    );
    let compact = serialize(&response, Detail::Compact).unwrap();
    let anchor = &compact["results"][0]["matches"][0];
    assert_eq!(anchor["message_id"], "message");
    assert_eq!(anchor["node_id"], "peer");
    assert_eq!(anchor["copies"][0]["message_id"], "copy");
    assert_eq!(anchor["snippet_truncated"], true);
    assert!(anchor["snippet"].as_str().unwrap().len() <= EXCERPT_CHARS + 3);
    assert!(anchor.get("fusion_score").is_none());
    assert_eq!(compact["results"][0]["score"], 0.5);
}

#[test]
fn compact_listing_keeps_pagination_and_view_never_truncates() {
    let response = Response::ListThreads(super::super::threads::Response {
        page: crate::thread_catalog::Page {
            threads: vec![crate::thread_catalog::Thread {
                conversation_id: "thread".into(),
                provider: crate::provider::Provider::Codex,
                node_id: "peer".into(),
                metadata: ThreadMetadata::default(),
            }],
            next_offset: Some(50),
        },
        shared: true,
        shared_error: Some("offline peer".into()),
        diagnostics: None,
    });
    let compact = serialize(&response, Detail::Compact).unwrap();
    assert_eq!(compact["next_offset"], 50);
    assert_eq!(compact["threads"][0]["node_id"], "peer");
    assert_eq!(compact["shared_error"], "offline peer");
    let view = Response::View(
        serde_json::from_value(json!({"id":"message","messages":[{
            "id":"message","ordinal":0,"role":"user","content":"界".repeat(1500),"created_at":null
        }]}))
        .unwrap(),
    );
    assert_eq!(
        serialize(&view, Detail::Compact).unwrap(),
        serialize(&view, Detail::Extended).unwrap()
    );
}

#[test]
fn compact_shared_status_preserves_freshness_coverage_and_readiness() {
    let freshness: crate::shared::Freshness = serde_json::from_value(json!({
        "group_id":"group", "unpublished_nodes":["missing-peer"], "nodes":[{
            "node_id":"peer","instance":"instance","revision":4,
            "last_synced_at":"2026-10-08T12:00:00Z","last_updated_at":"2026-10-08T11:00:00Z",
            "conversations":2,"messages":20,"lexical_current":true,"gold_current":false
        }]
    }))
    .unwrap();
    let compact = serde_json::to_value(freshness.summary()).unwrap();
    assert_eq!(compact["unpublished_nodes"][0], "missing-peer");
    assert_eq!(compact["nodes"][0]["revision"], 4);
    assert_eq!(compact["nodes"][0]["gold_current"], false);
    assert_eq!(
        compact["nodes"][0]["last_synced_at"],
        "2026-10-08T12:00:00Z"
    );
}
