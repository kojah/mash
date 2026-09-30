use super::*;
use crate::{
    ingestion::{IndexOptions, index, source_stamp_for},
    storage::Storage,
};
use serde_json::{Value, json};
use std::io::Write;

fn header() -> Value {
    json!({"type":"session","version":3,"id":"pi-session","timestamp":"2026-09-30T10:00:00Z","cwd":"/work/cbioportal"})
}

fn entry(id: &str, parent: Option<&str>, mut data: Value) -> Value {
    let object = data.as_object_mut().unwrap();
    object.insert("id".into(), id.into());
    object.insert("parentId".into(), parent.into());
    object.insert("timestamp".into(), "2026-09-30T10:00:01Z".into());
    data
}

fn message(id: &str, parent: Option<&str>, role: &str, text: &str) -> Value {
    let content = if role == "user" {
        json!(text)
    } else {
        json!([{"type":"text","text":text}])
    };
    entry(
        id,
        parent,
        json!({"type":"message","message":{"role":role,"content":content,"timestamp":1_790_762_401_000_i64}}),
    )
}

fn fixture(path: &Path) {
    let mut file = File::create(path).unwrap();
    for record in [
        header(),
        message("root", None, "user", "Fix the cBioPortal importer"),
        entry(
            "model",
            Some("root"),
            json!({"type":"model_change","provider":"test","modelId":"test"}),
        ),
        entry(
            "left",
            Some("model"),
            json!({"type":"message","message":{"role":"assistant","content":[
            {"type":"text","text":"First branch repairs mutation parsing."},
            {"type":"thinking","thinking":"privatereasoningmarker"},
            {"type":"toolCall","id":"call","name":"read","arguments":{"secret":"privatetoolmarker"}}
        ],"timestamp":1_790_762_402_000_i64}}),
        ),
        message("tool", Some("left"), "toolResult", "privateoutputmarker"),
        message(
            "left-user",
            Some("tool"),
            "user",
            "Leftbranch has a different approach.",
        ),
        message(
            "left-answer",
            Some("left-user"),
            "assistant",
            "Leftbranch uses an earlier parser.",
        ),
        message(
            "right",
            Some("root"),
            "user",
            "Rightbranch should validate mutation metadata.",
        ),
        message(
            "right-answer",
            Some("right"),
            "assistant",
            "Rightbranch validates the schema.",
        ),
        entry(
            "name",
            Some("right-answer"),
            json!({"type":"session_info","name":"Importer alternatives"}),
        ),
        entry(
            "hidden",
            Some("name"),
            json!({"type":"custom_message","content":"hiddenextensionmarker","display":false}),
        ),
        entry(
            "visible",
            Some("hidden"),
            json!({"type":"custom_message","content":"visibleextensionmarker","display":true}),
        ),
        entry(
            "compact",
            Some("visible"),
            json!({"type":"compaction","summary":"privatecompactionmarker","firstKeptEntryId":"right","tokensBefore":1200}),
        ),
        entry(
            "edit",
            Some("compact"),
            json!({"type":"context_edit","targetId":"root","replacement":null}),
        ),
        message(
            "after",
            Some("edit"),
            "user",
            "Continue the importer checks after compaction.",
        ),
    ] {
        writeln!(file, "{record}").unwrap();
    }
}

fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Pi, vec![path.to_path_buf()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

#[test]
fn native_tree_keeps_unique_branches_and_context_without_model_only_edits() {
    let file = tempfile::NamedTempFile::new().unwrap();
    fixture(file.path());
    let before = std::fs::read(file.path()).unwrap();
    let source = SourceIdentity::file(file.path());
    let stamp = source_stamp_for(&source).unwrap();
    let parsed = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(parsed.messages.len(), 10);
    assert_eq!(
        parsed.id,
        super::super::parsers::stable_id("session", &["pi", "pi-session"])
    );
    assert_eq!(parsed.title.as_deref(), Some("Importer alternatives"));
    assert_eq!(parsed.project.as_deref(), Some("/work/cbioportal"));
    assert!(parsed.repository.is_none());
    assert_eq!(parsed.messages.inline()[0].context, MessageContext::Root);
    assert_eq!(
        parsed.messages.inline()[1].context,
        MessageContext::Parent { ordinal: 0 }
    );
    assert_eq!(
        parsed.messages.inline()[5].context,
        MessageContext::Parent { ordinal: 0 }
    );
    assert_eq!(
        parsed.messages.inline()[7].context,
        MessageContext::Parent { ordinal: 6 }
    );
    assert_eq!(
        parsed.messages.inline()[9].context,
        MessageContext::Parent { ordinal: 8 }
    );
    let canonical = parsed
        .messages
        .inline()
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let searchable = parsed
        .messages
        .inline()
        .iter()
        .map(|m| m.search_projection.as_deref().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    for marker in [
        "privatereasoningmarker",
        "privatetoolmarker",
        "privateoutputmarker",
        "privatecompactionmarker",
        "visibleextensionmarker",
    ] {
        assert!(canonical.contains(marker));
        assert!(!searchable.contains(marker));
    }
    assert!(!canonical.contains("hiddenextensionmarker"));
    for text in ["cBioPortal", "Leftbranch", "Rightbranch"] {
        assert!(searchable.contains(text));
    }
    let repeated = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(
        parsed
            .messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>(),
        repeated
            .messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(std::fs::read(file.path()).unwrap(), before);
    stamp.verify(&source).unwrap();
}

#[test]
fn local_view_and_reranking_follow_branch_edges_instead_of_adjacent_siblings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    fixture(&path);
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 10);
    assert_eq!(storage.counts().unwrap().searchable_messages, 7);
    let parsed = parse(&path).unwrap().conversation.unwrap();
    storage
        .with_publication_snapshot(|snapshot| {
            snapshot.visit(crate::shared::protocol::BatchKind::Messages, |batch| {
                let crate::shared::protocol::Batch::Messages { records } = batch else {
                    panic!("message batch")
                };
                assert_eq!(records.len(), 10);
                for record in records {
                    let native = parsed
                        .messages
                        .inline()
                        .iter()
                        .find(|message| message.id == record.id)
                        .unwrap();
                    assert_eq!(record.context, native.context);
                }
                Ok(())
            })
        })
        .unwrap();
    let hits = storage
        .search("Rightbranch", 10, Some(Provider::Pi), None)
        .unwrap();
    assert_eq!(hits.len(), 2);
    let request = hits.iter().find(|h| h.role == Role::User).unwrap();
    for representation in [
        crate::storage::MessageContent::Canonical,
        crate::storage::MessageContent::SearchProjection,
    ] {
        let context = storage
            .view_content(&request.id, 1, representation)
            .unwrap();
        assert_eq!(
            context.iter().map(|m| m.ordinal).collect::<Vec<_>>(),
            [0, 5, 6]
        );
        let document = crate::search_results::contextual_document(request, "mutation", &context);
        assert!(document.contains("cBioPortal"));
        assert!(!document.contains("Leftbranch"));
    }
    let root = storage
        .search("cBioPortal", 10, None, None)
        .unwrap()
        .remove(0);
    assert_eq!(
        storage
            .view(&root.id, 2)
            .unwrap()
            .iter()
            .map(|m| m.ordinal)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(storage.view(&request.id, 0).unwrap().len(), 1);
}

#[test]
fn invalid_native_trees_fail_closed_and_cleared_titles_fall_back_to_user_text() {
    let file = tempfile::NamedTempFile::new().unwrap();
    for invalid in [
        message("bad", Some("missing"), "user", "orphan"),
        message("root", Some("root"), "user", "duplicate"),
        entry(
            "bad",
            Some("root"),
            json!({"type":"message","message":{"role":"user","content":42}}),
        ),
        json!({"type":"message","id":"bad","timestamp":"2026-09-30T10:00:00Z","message":{"role":"user","content":"missing parentId"}}),
        json!({"type":"message","id":"bad","parentId":"root","timestamp":"bad","message":{"role":"user","content":"invalid time"}}),
    ] {
        std::fs::write(
            file.path(),
            format!(
                "{}\n{}\n{invalid}\n",
                header(),
                message("root", None, "user", "original request")
            ),
        )
        .unwrap();
        assert!(parse(file.path()).is_err(), "{invalid}");
    }
    for version in [None, Some(1), Some(2), Some(4)] {
        let mut h = header();
        h["version"] = version.into();
        std::fs::write(file.path(), format!("{h}\n")).unwrap();
        let error = parse(file.path()).err().unwrap();
        assert!(error.error.message.contains("version"));
    }
    fixture(file.path());
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(file.path())
            .unwrap(),
        "{}",
        entry(
            "clear",
            Some("after"),
            json!({"type":"session_info","name":"  "})
        )
    )
    .unwrap();
    assert_eq!(
        parse(file.path())
            .unwrap()
            .conversation
            .unwrap()
            .title
            .as_deref(),
        Some("Fix the cBioPortal importer")
    );
}

#[test]
fn incomplete_refresh_preserves_checkpoint_and_retries_without_branch_duplicates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    fixture(&path);
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let stamp = source_stamp_for(&SourceIdentity::file(&path)).unwrap();
    let complete = std::fs::read(&path).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"type\":\"message\"")
        .unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(index(&mut storage, &options(&path)).is_err());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 10);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Pi, &SourceIdentity::file(&path), &stamp)
            .unwrap()
    );
    drop(storage);
    std::fs::write(&path, complete).unwrap();
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap(),
        "{}",
        message(
            "third",
            Some("root"),
            "user",
            "Thirdbranch independent retry"
        )
    )
    .unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert_eq!(
        index(&mut storage, &options(&path))
            .unwrap()
            .changed_messages,
        1
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 11);
    let hit = storage
        .search("Thirdbranch", 10, None, None)
        .unwrap()
        .remove(0);
    assert_eq!(
        storage
            .view(&hit.id, 2)
            .unwrap()
            .iter()
            .map(|m| m.ordinal)
            .collect::<Vec<_>>(),
        [0, 10]
    );
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn core_indexes_and_searches_all_native_pi_branches_with_real_models() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    let directory = tempfile::tempdir().unwrap();
    let history = directory.path().join("pi.jsonl");
    fixture(&history);
    let config = directory.path().join("config.toml");
    std::fs::write(&config, toml::to_string(&toml::toml!{ version=1 providers={pi={roots=[(history.to_str().unwrap())]}} index={all_history=true}}).unwrap()).unwrap();
    let paths = Paths {
        config: Some(config),
        db: Some(directory.path().join("index.sqlite3")),
        models_dir: Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into()),
    };
    let Response::Index(_) = crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .unwrap() else {
        panic!("index response")
    };
    for query in [
        "cbioportal importer",
        "leftbranch earlier parser",
        "rightbranch mutation metadata",
    ] {
        let Response::Search(search) = crate::core::execute(Request {
            paths: paths.clone(),
            operation: Operation::Search {
                query: query.into(),
                limit: 5,
                unit: crate::search_results::SearchUnit::Threads,
                filters: Box::new(crate::search_filter::Selection {
                    provider: vec![Provider::Pi],
                    ..Default::default()
                }),
                shared: false,
                local: true,
            },
        })
        .await
        .unwrap() else {
            panic!("search response")
        };
        let crate::search_results::Results::Threads(threads) = search.results else {
            panic!("threads")
        };
        assert_eq!(threads.len(), 1, "{query}");
        assert_eq!(threads[0].provider, Provider::Pi);
        let Response::View(view) = crate::core::execute(Request {
            paths: paths.clone(),
            operation: Operation::View {
                id: threads[0].matches[0].message_id.clone(),
                context: 2,
                shared: None,
            },
        })
        .await
        .unwrap() else {
            panic!("view response")
        };
        assert!(!view.messages.is_empty());
    }
}
