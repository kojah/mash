use super::*;

#[test]
fn view_returns_structured_tool_calls_and_results() {
    let root = tempfile::tempdir().unwrap();
    let provider = root.path().join("provider");
    std::fs::create_dir(&provider).unwrap();
    std::fs::write(
        provider.join("tools.jsonl"),
        include_str!("../../fixtures/claude-native-tools-2.1.296.jsonl"),
    )
    .unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=[{}]\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n",
        toml::Value::String(provider.to_str().unwrap().into()))).unwrap();
    let database = root.path().join("index.sqlite3");
    let models = root.path().join("no-models");
    let args = [
        "--config",
        config.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
    ];
    run_json(&[args.as_slice(), &["index"]].concat());
    let found = run_json(
        &[
            args.as_slice(),
            &[
                "search",
                "Checking both",
                "--local",
                "--no-worker",
                "--unit",
                "messages",
            ],
        ]
        .concat(),
    );
    let id = found["results"][0]["id"].as_str().unwrap().to_owned();
    let view = run_json(&[args.as_slice(), &["view", &id, "--context", "2"]].concat());
    let messages = view["messages"].as_array().unwrap();
    let activity = messages
        .iter()
        .filter_map(|message| message.get("tools").cloned())
        .collect::<Vec<_>>();
    assert_eq!(activity.len(), 3, "{view}");
    assert_eq!(
        activity[0]["tool_calls"][0],
        serde_json::json!({"name": "Bash", "call_id": "toolu_01A", "input": {"command": "ls fixtures", "description": "List fixtures"}})
    );
    assert_eq!(activity[0]["tool_calls"][1]["name"], "mcp__mash__search");
    assert_eq!(
        activity[1]["tool_results"],
        serde_json::json!([{"call_id": "toolu_01A", "is_error": false}, {"call_id": "toolu_01B", "is_error": true}])
    );
    assert_eq!(
        activity[2]["tool_results"],
        serde_json::json!([{"call_id": "toolu_01C"}])
    );
    // Entries without tool activity omit the field.
    assert!(
        messages
            .iter()
            .any(|message| message.get("tools").is_none())
    );
}
