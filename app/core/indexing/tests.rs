use super::*;

#[test]
fn embedding_progress_is_a_monotonic_newline_delimited_json_stream() {
    let first = semantic::EmbeddingProgress {
        stored_vectors: 32,
        total_vectors: 128,
        model_inferences: 24,
        reused_vectors: 8,
        elapsed_milliseconds: 1_000,
        stored_vectors_per_second: 32.0,
        eta_seconds: Some(3),
    };
    let second = semantic::EmbeddingProgress {
        stored_vectors: 64,
        total_vectors: 128,
        model_inferences: 48,
        reused_vectors: 16,
        elapsed_milliseconds: 2_000,
        stored_vectors_per_second: 32.0,
        eta_seconds: Some(2),
    };
    let mut output = Vec::new();

    write_embedding_progress(&mut output, &first).expect("first progress JSON");
    write_embedding_progress(&mut output, &second).expect("second progress JSON");

    assert_eq!(output.last(), Some(&b'\n'));
    let values = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<crate::progress::Event>(line).expect("progress JSON"))
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    assert!(values[0].stored_vectors.unwrap() < values[1].stored_vectors.unwrap());
    assert!(values[0].model_inferences.unwrap() < values[1].model_inferences.unwrap());
    assert_eq!(values[0].total_vectors, values[1].total_vectors);
    let value = &values[1];
    assert_eq!(
        serde_json::to_string(&value.kind).unwrap(),
        "\"index-progress\""
    );
    assert_eq!(value.phase, crate::progress::Phase::SemanticEmbeddings);
    assert_eq!(value.stored_vectors, Some(64));
    assert_eq!(value.total_vectors, Some(128));
    assert_eq!(value.model_inferences, Some(48));
    assert_eq!(value.reused_vectors, Some(16));
    assert_eq!(value.elapsed_milliseconds, 2_000);
    assert!((value.stored_vectors_per_second.unwrap() - 32.0).abs() < f64::EPSILON);
    assert_eq!(value.eta_seconds, Some(2));
}

fn test_configuration(
    claude_enabled: bool,
    codex_enabled: bool,
    since_days: Option<u32>,
) -> ResolvedConfig {
    let root = std::env::temp_dir().join("mash-cli-options");
    ResolvedConfig {
        qdrant: config::QdrantConfig::default(),
        attachments: crate::attachments::Configuration::default(),
        redaction: crate::redaction::Settings::default(),

        views: crate::views::config::Configuration::default(),
        path: root.join("config.toml"),
        loaded: true,
        shared: None,
        providers: [
            (Provider::ClaudeCode, claude_enabled, "claude"),
            (Provider::Codex, codex_enabled, "codex"),
        ]
        .into_iter()
        .filter(|(_, enabled, _)| *enabled)
        .map(|(provider, _, name)| (provider, vec![root.join(name)]))
        .collect(),
        since_days,
        destination: config::IndexDestination::Local,
    }
}

#[test]
fn index_options_validate_deduplicate_and_apply_horizon_precedence() {
    let configuration = test_configuration(true, true, Some(90));
    let selected = resolve_index_options(
        &configuration,
        &[Provider::Codex, Provider::Codex],
        Some(30),
        false,
    )
    .expect("selected options");
    assert!(!selected.providers.contains_key(&Provider::ClaudeCode));
    assert_eq!(selected.providers[&Provider::Codex].len(), 1);
    assert_eq!(selected.since_days, Some(30));

    let all = resolve_index_options(&configuration, &[], None, true).expect("all-history options");
    assert_eq!(all.since_days, None);
    assert!(all.providers.contains_key(&Provider::ClaudeCode));
    assert!(all.providers.contains_key(&Provider::Codex));

    for invalid in [Some(0), Some(36_501)] {
        assert!(resolve_index_options(&configuration, &[], invalid, false).is_err());
    }
    assert!(resolve_index_options(&configuration, &[], Some(1), true).is_err());
    assert_eq!("opencode".parse::<Provider>().unwrap(), Provider::Opencode);
    assert!("unsupported-provider".parse::<Provider>().is_err());
    assert!(
        resolve_index_options(
            &test_configuration(false, true, Some(90)),
            &[Provider::ClaudeCode],
            None,
            false,
        )
        .is_err()
    );
}
