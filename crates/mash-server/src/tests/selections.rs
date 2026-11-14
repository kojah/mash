use super::*;
use crate::provider::Provider;
use crate::search_filter::{Filter, Selection};
use crate::shared::protocol::{Candidates, Search};

pub(super) async fn verify_thread_provider_scopes(service: &Service, vector: &[f64]) {
    let selections = [
        (vec![], vec!["codex", "claude-code"]),
        (vec!["codex", "claude-code"], vec!["codex", "claude-code"]),
        (vec!["codex", "hermes", "unknown"], vec!["codex"]),
        (vec!["unknown"], vec![]),
    ];
    for (threads, expected) in selections {
        let scope = Selection {
            provider: vec![Provider::Codex, Provider::ClaudeCode],
            chat: threads.into_iter().map(str::to_owned).collect(),
            ..Default::default()
        }
        .resolve("first")
        .unwrap();
        let mut input = Search {
            match_mode: crate::views::matching::Mode::Strict,
            intent: crate::views::exact::Intent::RelatedDiscussion,
            vector_policy: None,
            unit: crate::search_results::SearchUnit::Messages,
            query: "providerneedle".into(),
            generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
            vector: vector.to_vec(),
            provider: None,
            days: None,
            exclude_self: false,
            filter: Some(scope.filter),
        };
        assert_candidates(service, FIRST, &input, &expected).await;
        assert_candidates(service, OTHER, &input, &[]).await;
        let original = input.filter.take().unwrap();
        input.filter = Some(Filter::Chat(vec![]));
        assert_candidates(service, FIRST, &input, &[]).await;
        input.filter = Some(Filter::All(vec![original, Filter::After(1)]));
        // These records lack timestamps; a positive time predicate excludes them.
        assert_candidates(service, FIRST, &input, &[]).await;
    }
}

async fn assert_candidates(service: &Service, token: &str, input: &Search, expected: &[&str]) {
    let result: Candidates = service
        .request("search", token)
        .json(input)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    for candidates in [result.lexical, result.semantic] {
        let mut ids: Vec<_> = candidates.into_iter().map(|hit| hit.chat_id).collect();
        ids.sort();
        let mut expected = expected.to_vec();
        expected.sort_unstable();
        assert_eq!(ids, expected);
    }
}
