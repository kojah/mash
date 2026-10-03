use super::*;
use crate::silver::DirectScan;

// Native legacy-task-handling.ts appends this warning after applying the API
// deleted-range projection. The visible UI prefix remains untruncated.
pub(super) fn resumed() -> Value {
    let mut value = sdk_fixture();
    value["messages"] = json!([
        {"role":"user","content":"privateimportedmodelcopymarker"},
        {"role":"assistant","content":"privateimportedanswermarker"},
        {"role":"user","content":super::super::migration::RESUME_WARNING},
        {"role":"user","content":"Run cBioPortal validation again."},
        {"role":"assistant","content":[{"type":"tool_use","id":"done","name":"submit_and_exit","input":{"summary":"The importer now validates structural variants."}}]},
        {"role":"user","content":"Run cBioPortal validation again."}
    ]);
    value
}

fn scan(path: &Path) -> DirectScan {
    direct_scans(&options(path)).unwrap().remove(0)
}
fn load(scan: &DirectScan) -> Conversation {
    let source = &scan.sources[0];
    DirectScan::parse_source(Provider::Cline, source, &source.stamp().unwrap())
        .unwrap()
        .conversation
        .unwrap()
}

#[test]
fn native_resume_composes_the_ui_prefix_once_for_catalogs_and_direct_artifacts() {
    for catalog_backed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let ui = ui_path(dir.path());
        write(&ui, &ui_fixture());
        let artifact = sdk_path(dir.path());
        write(&artifact, &resumed());
        let connection = catalog_backed.then(|| catalog(dir.path(), &artifact).1);
        let initial = scan(dir.path());
        assert_eq!(initial.sources.len(), 1);
        let conversation = load(&initial);
        let native_ui = parsed(&ui);
        assert_eq!(conversation.messages.len(), native_ui.messages.len() + 3);
        assert_eq!(conversation.created_at, Some(1));
        for (merged, old) in conversation
            .messages
            .inline()
            .iter()
            .zip(native_ui.messages.inline())
        {
            assert_eq!(merged.id, old.id);
            assert_eq!(merged.content, old.content);
        }
        let suffix = &conversation.messages.inline()[native_ui.messages.len()..];
        assert_ne!(
            suffix[0].id, suffix[2].id,
            "identical repeated requests remain distinct"
        );
        assert_eq!(suffix[0].content, suffix[2].content);
        assert!(
            conversation
                .messages
                .inline()
                .iter()
                .all(|m| !m.content.contains("privateimported")
                    && !m.content.contains(super::super::migration::RESUME_WARNING))
        );
        assert!(
            initial.sources[0]
                .dependencies
                .iter()
                .any(|d| d.path() == ui)
        );
        assert!(crate::silver::watch_roots(&options(dir.path())).contains(&ui));
        let ids = conversation
            .messages
            .inline()
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>();
        let mut refreshed = resumed();
        for (i, m) in refreshed["messages"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            m["id"] = json!(format!("restored-{i}"));
            m["ts"] = json!(1000 + i);
        }
        write(&artifact, &refreshed);
        assert_eq!(
            load(&scan(dir.path()))
                .messages
                .inline()
                .iter()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        // Cline keeps model compaction in a disposable sidecar; it must not
        // replace canonical history or the preserved legacy UI prefix.
        write(
            &artifact.with_file_name("task-1.compaction.json"),
            &json!({"version":1,"updated_at":"2026-09-30T10:00:02Z","source_message_count":6,"messages":[{"role":"user","content":"privatecompactionmarker"}]}),
        );
        assert_eq!(
            load(&scan(dir.path()))
                .messages
                .inline()
                .iter()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        if let Some(connection) = connection {
            connection
                .execute("UPDATE sessions SET metadata_json='{}'", [])
                .unwrap();
        }
        assert_eq!(
            load(&scan(dir.path())).messages.len(),
            native_ui.messages.len() + 3
        );
    }
}

#[test]
fn absent_or_hidden_resume_markers_use_sdk_history_without_a_guessed_union() {
    let dir = tempfile::tempdir().unwrap();
    let ui = ui_path(dir.path());
    write(&ui, &ui_fixture());
    let artifact = sdk_path(dir.path());
    let mut sdk = sdk_fixture();
    write(&artifact, &sdk);
    let conversation = load(&scan(dir.path()));
    assert_eq!(
        conversation.messages.len(),
        parsed(&artifact).messages.len()
    );
    assert_eq!(
        conversation
            .messages
            .inline()
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>(),
        parsed(&artifact)
            .messages
            .inline()
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>()
    );
    sdk["messages"][0] = json!({"role":"user","metadata":{"displayRole":"system"},"content":super::super::migration::RESUME_WARNING});
    write(&artifact, &sdk);
    let conversation = load(&scan(dir.path()));
    assert_eq!(conversation.messages.len(), 7);
    assert_eq!(
        conversation.messages.inline()[0].dialogue_text.as_deref(),
        Some("")
    );
}

#[test]
fn changes_to_composition_are_verified_even_when_the_primary_source_key_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = sdk_path(dir.path());
    write(&artifact, &resumed());
    let initial = scan(dir.path());
    let key = initial.sources[0].identity.key();
    let ui = ui_path(dir.path());
    write(&ui, &ui_fixture());
    let merged = scan(dir.path());
    assert_eq!(merged.sources[0].identity.key(), key);
    assert!(
        initial.verify().is_err(),
        "new legacy dependency must invalidate authoritative discovery"
    );
    let stamp = merged.sources[0].stamp().unwrap();
    std::fs::write(&ui, b"[{\"ts\":").unwrap();
    assert!(stamp.verify(&merged.sources[0].identity).is_err());
    assert!(load_result(&scan(dir.path())).is_err());
    std::fs::remove_file(&ui).unwrap();
    assert!(merged.verify().is_err());
}

fn load_result(scan: &DirectScan) -> Result<ParsedFile, AppError> {
    let source = &scan.sources[0];
    DirectScan::parse_source(Provider::Cline, source, &source.stamp()?)
}

#[test]
fn migration_keeps_the_previous_index_on_interrupted_dependency_updates_and_rewinds() {
    let dir = tempfile::tempdir().unwrap();
    let ui = ui_path(dir.path());
    write(&ui, &ui_fixture());
    let database = dir.path().join("mash.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(dir.path())).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let artifact = sdk_path(dir.path());
    write(&artifact, &resumed());
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(dir.path())).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().conversations, 1);
    assert_eq!(storage.counts().unwrap().messages, 11);
    assert!(
        storage
            .search("privateimportedmodelcopymarker", 10, None, None)
            .unwrap()
            .is_empty()
    );
    drop(storage);
    let initial = scan(dir.path());
    let stamp = initial.sources[0].stamp().unwrap();
    std::fs::write(&ui, b"[").unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(index(&mut storage, &options(dir.path())).is_err());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 11);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Cline, &initial.sources[0].identity, &stamp)
            .unwrap()
    );
    drop(storage);
    write(&ui, &ui_fixture());
    let mut rewind = resumed();
    rewind["messages"].as_array_mut().unwrap().pop();
    write(&artifact, &rewind);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(dir.path())).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 10);
    assert_eq!(
        storage
            .search("cbioportal", 20, None, None)
            .unwrap()
            .iter()
            .filter(|m| m.content == "Run cBioPortal validation again.")
            .count(),
        1
    );
}
