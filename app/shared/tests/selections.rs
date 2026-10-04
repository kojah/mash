use super::*;
use crate::provider::Provider;
use crate::search_filter::Selection;

#[test]
fn local_message_candidates_intersect_thread_provider_and_time_selections() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    for provider in [Provider::Codex, Provider::ClaudeCode, Provider::Opencode] {
        let id = provider.as_str();
        storage
            .replace_conversation(&Conversation {
                id: id.into(),
                provider,
                title_origin: None,
                timestamp_origins: None,
                claude_lineage: None,
                source: crate::silver::SourceIdentity::file(id),
                project: None,
                repository: None,
                title: None,
                created_at: Some(2000),
                updated_at: Some(2000),
                messages: vec![NormalizedMessage {
                    attachments: Vec::new(),
                    id: id.into(),
                    ordinal: 0,
                    context: crate::message_context::MessageContext::Linear,
                    role: crate::role::Role::User,
                    content: "providerneedle canonical history".into(),
                    dialogue_text: None,
                    created_at: Some(2000),
                }]
                .into(),
            })
            .unwrap();
        storage
            .replace_embeddings(
                semantic::embedding_generation(),
                &[EmbeddingWrite {
                    message_id: id,
                    vector: &[127, 0],
                    norm: 127.0,
                }],
            )
            .unwrap();
    }
    storage.commit_writer().unwrap();
    for (threads, expected) in [
        (vec![], vec!["codex", "claude-code"]),
        (vec!["codex", "claude-code"], vec!["codex", "claude-code"]),
        (vec!["codex", "hermes", "unknown"], vec!["codex"]),
        (vec!["unknown"], vec![]),
    ] {
        for after in [2, 3] {
            let scope = Selection {
                provider: vec![Provider::Codex, Provider::ClaudeCode],
                conversation: threads.iter().map(|id| (*id).into()).collect(),
                after: Some(after),
                before: Some(3),
                ..Default::default()
            }
            .resolve("local")
            .unwrap();
            let (hits, _) = semantic::local_candidates(
                &storage,
                semantic::embedding_generation(),
                "providerneedle",
                &[127.0, 0.0],
                (None, None, SearchUnit::Messages),
                Some(&scope),
                crate::gold::config::Fusion::default(),
            )
            .unwrap();
            let mut ids: Vec<_> = hits
                .into_iter()
                .map(|hit| {
                    assert!(hit.lexical_score.is_some() && hit.semantic_score.is_some());
                    hit.conversation_id
                })
                .collect();
            ids.sort();
            let mut expected = if after == 2 { expected.clone() } else { vec![] };
            expected.sort_unstable();
            assert_eq!(ids, expected);
        }
    }
}
