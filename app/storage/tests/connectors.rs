use super::*;
use crate::connectors::{AuthorKind, Batch};
use crate::source::Source;

const MESSAGE: &str = "connector#entry#github:org#review-1";
const REPLY: &str = "connector#entry#github:org#review-2";
const DOCUMENT: &str = "connector#document#github:org#note";
const CHAT: &str = "connector#container#github:org#pr-42";

fn batch(revision: u64, changes: &serde_json::Value) -> Batch {
    Batch::decode(
        serde_json::json!({
            "schema_version": 2, "source_id": "github:org",
            "batch_id": format!("batch-{revision}"), "expected_revision": revision,
            "changes": changes,
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap()
}

fn initial() -> Batch {
    batch(
        0,
        &serde_json::json!([
            {"op": "upsert-object", "id": "pr-42", "role": "container",
             "kind": "github:pull-request", "title": "Cache rollout",
             "created_at": 1000, "updated_at": 3000},
            {"op": "upsert-object", "id": "review", "role": "segment",
             "kind": "github:review", "parent": "pr-42", "title": "Changes requested"},
            {"op": "upsert-object", "id": "review-1", "role": "entry",
             "kind": "github:review-comment", "parent": "review",
             "content": "Please measure the zeppelin latency first",
             "author": {"id": "alice", "kind": "human"}, "created_at": 1000},
            {"op": "upsert-object", "id": "review-2", "role": "entry",
             "kind": "github:comment", "parent": "pr-42",
             "content": "Measured; results attached", "reply_to": "review-1",
             "author": {"id": "ci", "kind": "bot"}, "created_at": 2000},
            {"op": "upsert-object", "id": "note", "role": "document", "kind": "obsidian:note",
             "title": "Runbook",
             "content": "The zeppelin dashboard lives here", "created_at": 500},
        ]),
    )
}

/// Native and connector canonical history in the same silver schema.
fn database(root: &std::path::Path) -> std::path::PathBuf {
    let path = root.join("connectors.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    writer
        .replace_conversation(&conversation_with_messages(&[(
            "native",
            "native zeppelin discussion",
        )]))
        .unwrap();
    writer
        .begin_silver_scan(crate::provider::Provider::Codex)
        .unwrap();
    writer
        .finish_silver_scan(crate::provider::Provider::Codex, true)
        .unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    apply(&path, &initial());
    path
}

fn apply(path: &std::path::Path, batch: &Batch) {
    crate::connectors::silver::Store::open(path)
        .unwrap()
        .apply(batch)
        .unwrap();
}

fn context_windows(storage: &Storage, ids: &[&str], context: u32) -> Vec<Vec<Message>> {
    let mut reader = storage.context_reader();
    ids.iter()
        .map(|id| {
            reader
                .view_content(id, context, MessageContent::SearchProjection)
                .unwrap()
        })
        .collect()
}

#[test]
fn view_and_listing_resolve_connector_anchors() {
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    let reader = Storage::open_silver(&path).unwrap();
    let context = reader.view(MESSAGE, 1).unwrap();
    assert_eq!(
        context.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [MESSAGE, REPLY]
    );
    assert_eq!(context[1].author_kind, Some(AuthorKind::Bot));
    assert_eq!(context[1].role, None);
    assert_eq!(reader.view(REPLY, 0).unwrap()[0].ordinal, 1);
    let document = reader.view(DOCUMENT, 2).unwrap();
    assert_eq!(document.len(), 1);
    assert_eq!(
        document[0].content,
        "Runbook\nThe zeppelin dashboard lives here"
    );
    assert!(
        reader
            .view("connector#message#github:org#missing", 1)
            .unwrap()
            .is_empty()
    );
    let catalog = Storage::open_silver(&path).unwrap();
    let chats = catalog
        .chat_candidates(&crate::chat_catalog::Request::default(), "local")
        .unwrap();
    let connector = chats
        .iter()
        .find(|chat| chat.conversation_id == CHAT)
        .unwrap();
    assert_eq!(connector.source, Source::Connector("github:org".into()));
    assert_eq!(connector.metadata.title.as_deref(), Some("Cache rollout"));
    let native_only = catalog
        .chat_candidates(
            &crate::chat_catalog::Request {
                provider: vec![crate::provider::Provider::Codex],
                ..crate::chat_catalog::Request::default()
            },
            "local",
        )
        .unwrap();
    assert!(native_only.iter().all(|chat| chat.conversation_id != CHAT));
}

#[test]
fn batched_context_reuses_order_preserves_windows_and_refreshes_between_calls() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    let reader = Storage::open_silver(&path).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&reads);
    reader
        .connection
        .authorizer(Some(move |context: AuthContext<'_>| {
            // The ordering query selects created_at; evidence queries select only
            // role, parent_id and record. Count actual SQL compilation, not a cache
            // field, so removing batching makes this assertion fail.
            if matches!(
                context.action,
                AuthAction::Read {
                    table_name: "objects",
                    column_name: "created_at"
                }
            ) {
                observed.fetch_add(1, Ordering::Relaxed);
            }
            Authorization::Allow
        }))
        .unwrap();
    let anchors = [REPLY, MESSAGE, REPLY];
    let contexts = context_windows(&reader, &anchors, 1);
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    for context in &contexts {
        assert_eq!(
            context
                .iter()
                .map(|m| (m.id.as_str(), m.ordinal))
                .collect::<Vec<_>>(),
            [(MESSAGE, 0), (REPLY, 1)]
        );
    }
    reads.store(0, Ordering::Relaxed);
    let singles = anchors
        .iter()
        .map(|id| {
            reader
                .view_content(id, 1, MessageContent::SearchProjection)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(reads.load(Ordering::Relaxed), 3);
    assert_eq!(
        serde_json::to_value(contexts).unwrap(),
        serde_json::to_value(singles).unwrap()
    );
    apply(
        &path,
        &batch(
            1,
            &serde_json::json!([
                {"op":"upsert-object", "id":"review-2", "role":"entry", "kind":"github:comment", "parent":"pr-42", "content":"Earlier corrected reply", "created_at":10, "author":{"id":"ci","kind":"bot"}}
            ]),
        ),
    );
    let refreshed = context_windows(&reader, &[REPLY, MESSAGE], 1);
    for context in refreshed {
        assert_eq!(
            context
                .iter()
                .map(|m| (m.id.as_str(), m.ordinal))
                .collect::<Vec<_>>(),
            [(REPLY, 0), (MESSAGE, 1)]
        );
        assert_eq!(context[0].content, "Earlier corrected reply");
    }
}

#[test]
fn batched_context_preserves_mixed_projections_documents_and_missing_anchors() {
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    {
        let mut writer = Storage::open_writer(&path).unwrap();
        let mut native = conversation_with_messages(&[("native", "native canonical text")]);
        native.messages.inline_mut()[0].dialogue_text = Some("native searchable projection".into());
        writer.replace_conversation(&native).unwrap();
        writer.commit_writer().unwrap();
    }
    let reader = Storage::open_silver(&path).unwrap();
    let mixed = [
        DOCUMENT,
        "native",
        "connector#entry#github:org#missing",
        CHAT,
        REPLY,
    ];
    let contexts = context_windows(&reader, &mixed, 0);
    assert_eq!(
        contexts[0][0].content,
        "Runbook\nThe zeppelin dashboard lives here"
    );
    assert_eq!(contexts[1][0].content, "native searchable projection");
    assert!(contexts[2].is_empty());
    assert!(contexts[3].is_empty());
    assert_eq!(contexts[4][0].id, REPLY);
    assert_eq!(contexts[4][0].ordinal, 1);
}

#[test]
fn index_only_counts_match_native_entries_beside_segments_and_connector_objects() {
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    let storage = Storage::open_silver(&path).unwrap();
    let direct = |predicate: &str| -> u64 {
        storage
            .connection
            .query_row(
                &format!("SELECT count(*) FROM objects NOT INDEXED WHERE {predicate}"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| u64::try_from(count).unwrap())
            .unwrap()
    };
    // The fixture holds native turns and every connector role, so a count
    // that leaked either into native entries would differ here.
    assert!(direct("origin = 'provider' AND object_role = 'segment'") > 0);
    assert!(
        direct("origin = 'connector' AND object_role IN ('segment', 'entry', 'document')") >= 4
    );
    let counts = storage.counts().unwrap();
    let entries = direct("origin = 'provider' AND object_role = 'entry'");
    assert_eq!(counts.messages, entries);
    assert_eq!(
        counts.conversations,
        direct("origin = 'provider' AND object_role = 'container'")
    );
    assert_eq!(
        counts.searchable_messages,
        direct(
            "origin = 'provider' AND object_role = 'entry' AND COALESCE(search_projection, content) <> ''"
        )
    );
}
