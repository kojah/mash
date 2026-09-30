use super::*;

#[test]
fn old_empty_window_location_and_plain_response_shapes_survive_migration() {
    let f = Fixture::new(true);
    let previous = f
        .dir
        .path()
        .join("workspaceStorage/no-workspace/chatSessions");
    std::fs::create_dir_all(&previous).unwrap();
    let mut snapshot = session();
    snapshot.as_object_mut().unwrap().remove("version");
    snapshot["requests"][0]["response"] = json!("Older plain answer");
    std::fs::write(previous.join("chat-session.json"), snapshot.to_string()).unwrap();
    let original = f.parse().unwrap();
    assert_eq!(
        original.messages.inline()[1].search_projection.as_deref(),
        Some("Older plain answer")
    );
    assert!(crate::ingestion::watch_roots(&f.options()).contains(&previous));
    let source = f.sources().unwrap().remove(0);
    let stamp = source.stamp().unwrap();
    snapshot["version"] = json!(3);
    snapshot["requests"][0]["response"] = json!(["First answer", {"value":"Second answer"}]);
    f.write(
        "jsonl",
        &log(&[
            json!({"kind":0,"v":snapshot}),
            json!({"kind":2,"k":["requests",0,"response"],"v":["Appended answer"]}),
        ]),
    );
    assert!(stamp.verify(&source.identity).is_err());
    let migrated = f.parse().unwrap();
    assert_eq!(original.id, migrated.id);
    assert_eq!(original.source.key(), migrated.source.key());
    assert_eq!(
        original.messages.inline()[1].id,
        migrated.messages.inline()[1].id
    );
    assert_eq!(
        migrated.messages.inline()[1].search_projection.as_deref(),
        Some("First answer\nSecond answer\nAppended answer")
    );
}
