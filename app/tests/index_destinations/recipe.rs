use super::*;

fn write_source(fixture: &Fixture, title: &str) {
    std::fs::write(fixture.codex.join("session.jsonl"), format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"codex-session\"}}}}\n{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":{title:?}}}]}}}}\n"
    )).unwrap();
}

fn enable_claude(fixture: &Fixture, title: &str) {
    let mut config: toml::Value =
        toml::from_str(&std::fs::read_to_string(&fixture.config).unwrap()).unwrap();
    config["index"]["shared"]["providers"] = toml::Value::Array(
        ["codex", "claude-code"]
            .into_iter()
            .map(|value| toml::Value::String(value.into()))
            .collect(),
    );
    std::fs::write(&fixture.config, toml::to_string(&config).unwrap()).unwrap();
    std::fs::write(fixture.directory.path().join("claude/session.jsonl"), format!(
        "{{\"type\":\"user\",\"sessionId\":\"claude-session\",\"uuid\":\"claude-user\",\"message\":{{\"role\":\"user\",\"content\":{title:?}}}}}\n"
    )).unwrap();
}

fn search_scores(fixture: &Fixture) -> std::collections::BTreeMap<String, f64> {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut vector = vec![0.0; 384];
        vector[0] = 127.0;
        let result: Value = reqwest::Client::new()
            .post(format!("{}/v1/search", fixture.server.as_ref().unwrap().url))
            .bearer_auth(shared_server::token("split"))
            .json(&serde_json::json!({"query":"canonical", "vector":vector,"provider":null,"days":null,"unit":"messages"}))
            .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        result["semantic"].as_array().unwrap().iter().map(|hit| {
            (hit["id"].as_str().unwrap().to_owned(), hit["semantic_score"].as_f64().unwrap())
        }).collect()
    })
}

fn assert_current(fixture: &Fixture, messages: i64) -> Value {
    let state = fixture.remote_node("split");
    assert_eq!(state["gold_current"], true);
    assert_eq!(state["lexical_current"], true);
    assert_eq!(state["messages"], messages);
    assert_eq!(
        state["gold_manifest"]["build"]["embedding_text"],
        "title-and-dialogue"
    );
    state
}

fn unfiltered(fixture: &Fixture) {
    let mut config: toml::Value =
        toml::from_str(&std::fs::read_to_string(&fixture.config).unwrap()).unwrap();
    config["index"] = toml::Value::Table(toml::toml! { destination = "both" });
    config["shared"]["api_url"] = toml::Value::String(fixture.server.as_ref().unwrap().url.clone());
    std::fs::write(&fixture.config, toml::to_string(&config).unwrap()).unwrap();
}

fn local_witness(fixture: &Fixture) -> Vec<String> {
    let connection = rusqlite::Connection::open_with_flags(
        &fixture.database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    [
        "SELECT json_object('id',id,'provider',provider,'title',title,'project',project,'repository',repository,'created_at',created_at,'updated_at',updated_at) AS witness FROM conversations ORDER BY id",
        "SELECT json_object('id',id,'conversation',conversation_id,'ordinal',ordinal,'role',role,'content',content,'projection',search_projection,'parent',context_parent,'created_at',created_at) AS witness FROM messages ORDER BY id",
        "SELECT json_object('id',conversation_id,'evidence',evidence) AS witness FROM conversation_enrichment ORDER BY conversation_id",
        "SELECT json_object('id',message_id,'generation',generation,'vector',hex(vector),'norm',norm) AS witness FROM message_embeddings ORDER BY message_id",
    ].into_iter().flat_map(|sql| {
        connection.prepare(sql).unwrap().query_map([], |row| row.get::<_, String>("witness")).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap()
    }).collect()
}

#[test]
#[ignore = "requires disposable Qdrant and real models"]
fn qdrant_full_snapshots_negotiate_recipes_reuse_unchanged_threads_and_forget_without_sources() {
    let mut fixture = Fixture::new();
    fixture.models = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into();
    fixture.configure("both", "split");
    enable_claude(&fixture, "canonical Claude topic about retrieval");
    write_source(&fixture, "canonical Codex topic about compilation");
    unfiltered(&fixture);
    let initial = fixture.json(&["index", "--all-history"], true);
    assert_eq!(initial["shared"]["state"], "published");
    assert_eq!(initial["shared"]["preparation"]["forwarded_embeddings"], 2);
    assert_eq!(initial["shared"]["preparation"]["model_inferences"], 0);
    let before = fixture.remote_node("split");
    let local = local_witness(&fixture);
    let dialogue_scores = search_scores(&fixture);
    drop(fixture.server.take());
    fixture.server = Some(shared_server::Server::with_recipe(
        fixture.directory.path(),
        &fixture.group,
        &["split"],
        "title-and-dialogue",
    ));
    unfiltered(&fixture);
    let switched = fixture.json(&["index", "--all-history"], true);
    assert_eq!(switched["shared"]["state"], "published");
    assert_eq!(switched["shared"]["preparation"]["model_inferences"], 2);
    assert_eq!(
        assert_current(&fixture, 2)["silver_revision"],
        before["silver_revision"]
    );
    assert_eq!(local_witness(&fixture), local);
    let title_scores = search_scores(&fixture);
    assert_ne!(title_scores, dialogue_scores);
    let noop = fixture.json(&["index", "--all-history"], true);
    assert_eq!(noop["shared"]["state"], "published");
    assert_eq!(noop["shared"]["preparation"]["model_inferences"], 0);
    assert_eq!(noop["shared"]["preparation"]["reused_threads"], 2);
    assert_eq!(noop["shared"]["preparation"]["reused_embeddings"], 2);
    assert_eq!(search_scores(&fixture), title_scores);
    write_source(&fixture, "canonical updated compilation topic");
    let edited = fixture.json(&["index", "--all-history"], true);
    assert_eq!(edited["shared"]["state"], "published");
    assert_eq!(edited["shared"]["preparation"]["model_inferences"], 1);
    assert_eq!(edited["shared"]["preparation"]["reused_threads"], 1);
    let scores = search_scores(&fixture);
    fixture.json(&["index", "--rebuild-shared-gold"], true);
    assert_eq!(search_scores(&fixture), scores);
    std::fs::remove_file(fixture.codex.join("session.jsonl")).unwrap();
    std::fs::remove_file(fixture.directory.path().join("claude/session.jsonl")).unwrap();
    let connection = rusqlite::Connection::open(&fixture.database).unwrap();
    let id: String = connection
        .query_row(
            "SELECT id FROM conversations WHERE provider='codex'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    let forgotten = fixture.json(&["forget", &id], true);
    assert_eq!(forgotten["shared"]["state"], "published");
    assert_eq!(forgotten["shared"]["preparation"]["model_inferences"], 0);
    assert_eq!(forgotten["shared"]["preparation"]["reused_threads"], 1);
    assert_current(&fixture, 1);
    eprintln!(
        "snapshot recipe diagnostic: default={}ms switch={}ms noop={}ms edit={}ms; preparation/default={} switch={} noop={} edit={}",
        initial["total_milliseconds"],
        switched["total_milliseconds"],
        noop["total_milliseconds"],
        edited["total_milliseconds"],
        initial["shared"]["preparation"],
        switched["shared"]["preparation"],
        noop["shared"]["preparation"],
        edited["shared"]["preparation"]
    );
    fixture.cleanup();
}

#[test]
#[ignore = "requires disposable Qdrant and real models"]
fn qdrant_routed_ingestion_preserves_active_recipe_across_edits_noops_and_deletions() {
    let mut fixture = Fixture::new();
    fixture.models = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into();
    drop(fixture.server.take());
    fixture.server = Some(shared_server::Server::with_recipe(
        fixture.directory.path(),
        &fixture.group,
        &["split"],
        "title-and-dialogue",
    ));
    fixture.configure("both", "split");
    enable_claude(&fixture, "canonical Claude initial topic");
    write_source(&fixture, "canonical initial topic");
    let initial = fixture.json(&["index", "--all-history"], true);
    assert_eq!(initial["shared"]["state"], "published");
    assert_eq!(initial["gold"]["build"]["embedding_text"], "dialogue");
    let before = assert_current(&fixture, 2);
    let scores = search_scores(&fixture);
    let unchanged = fixture.json(&["index", "--all-history"], true);
    assert_eq!(unchanged["shared"]["state"], "published");
    assert_eq!(
        assert_current(&fixture, 2)["silver_revision"],
        before["silver_revision"]
    );
    assert_eq!(search_scores(&fixture), scores);
    let rebuilt = fixture.json(&["index", "--rebuild-shared-gold"], true);
    assert_eq!(rebuilt["model_inferences"], 2);
    assert_eq!(
        search_scores(&fixture),
        scores,
        "incremental and retained-silver builders disagree"
    );
    write_source(&fixture, "canonical updated topic about compilation");
    let changed = fixture.json(&["index", "--all-history"], true);
    assert_eq!(changed["shared"]["state"], "published");
    let after = assert_current(&fixture, 2);
    assert_ne!(after["silver_revision"], before["silver_revision"]);
    let edited_scores = search_scores(&fixture);
    assert_ne!(edited_scores, scores);
    fixture.json(&["index", "--rebuild-shared-gold"], true);
    assert_eq!(search_scores(&fixture), edited_scores);
    enable_claude(&fixture, "canonical Claude updated topic about retrieval");
    fixture.json(&["index", "--all-history"], true);
    assert_current(&fixture, 2);
    let claude_scores = search_scores(&fixture);
    assert_ne!(claude_scores, edited_scores);
    fixture.json(&["index", "--rebuild-shared-gold"], true);
    assert_eq!(search_scores(&fixture), claude_scores);
    std::fs::remove_file(fixture.codex.join("session.jsonl")).unwrap();
    std::fs::remove_file(fixture.directory.path().join("claude/session.jsonl")).unwrap();
    let deleted = fixture.json(&["index", "--all-history"], true);
    assert_eq!(deleted["shared"]["state"], "published");
    let empty = assert_current(&fixture, 0);
    fixture.json(&["index", "--all-history"], true);
    assert_eq!(
        assert_current(&fixture, 0)["silver_revision"],
        empty["silver_revision"]
    );
    eprintln!(
        "routed title recipe CLI fixture costs: initial={}ms, noop={}ms, edit={}ms, delete={}ms",
        initial["total_milliseconds"],
        unchanged["total_milliseconds"],
        changed["total_milliseconds"],
        deleted["total_milliseconds"]
    );
    fixture.cleanup();
}
