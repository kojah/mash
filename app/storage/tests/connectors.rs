use super::*;
use crate::connectors::{AuthorKind, Batch};
use crate::gold::exact::Literal;
use crate::search_filter::{Filter, Selection};
use crate::source::{Kind, Source};

const MESSAGE: &str = "connector#message#github:org#review-1";
const REPLY: &str = "connector#message#github:org#review-2";
const DOCUMENT: &str = "connector#document#github:org#note";
const CHAT: &str = "connector#chat#github:org#pr-42";

fn batch(revision: u64, changes: &serde_json::Value) -> Batch {
    Batch::decode(
        serde_json::json!({
            "schema_version": 1, "source_id": "github:org",
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
            {"op": "upsert-chat", "id": "pr-42", "title": "Cache rollout",
             "created_at": 1000, "updated_at": 3000},
            {"op": "upsert-message", "id": "review-1", "chat_id": "pr-42",
             "content": "Please measure the zeppelin latency first",
             "author": {"id": "alice", "kind": "human"}, "created_at": 1000},
            {"op": "upsert-message", "id": "review-2", "chat_id": "pr-42",
             "content": "Measured; results attached", "reply_to": "review-1",
             "author": {"id": "ci", "kind": "bot"}, "created_at": 2000},
            {"op": "upsert-document", "id": "note", "title": "Runbook",
             "content": "The zeppelin dashboard lives here", "created_at": 500},
        ]),
    )
}

/// Native silver plus certified lexical gold, then connector silver on top.
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
    writer.refresh_derived_search_state().unwrap();
    writer.publish_lexical_manifest().unwrap();
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

/// The gold stage: project connector silver and recertify lexical gold.
fn project(path: &std::path::Path) {
    let mut writer = Storage::open_writer(path).unwrap();
    writer.refresh_derived_search_state().unwrap();
    writer.publish_lexical_manifest().unwrap();
    writer.commit_writer().unwrap();
}

fn scope(selection: Selection) -> crate::search_filter::Scope {
    selection.resolve("local").unwrap()
}

fn ids(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.id.as_str()).collect()
}

#[test]
fn connector_commits_keep_search_closed_until_gold_projects_them() {
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    assert!(Storage::open_lexical(&path).is_err());
    project(&path);
    let reader = Storage::open_lexical(&path).unwrap();
    let all = scope(Selection::default());
    let hits = reader
        .search_filtered("zeppelin", 10, None, None, Some(&all))
        .unwrap();
    let mut found = ids(&hits);
    found.sort_unstable();
    assert_eq!(found, [DOCUMENT, MESSAGE, "native"]);
    let message = hits.iter().find(|hit| hit.id == MESSAGE).unwrap();
    assert_eq!(message.source, Source::Connector("github:org".into()));
    assert_eq!(message.kind, Kind::Message);
    assert_eq!(message.conversation_id, CHAT);
    assert_eq!(message.role, None);
    assert_eq!(message.author_kind, Some(AuthorKind::Human));
    assert_eq!(message.metadata.title.as_deref(), Some("Cache rollout"));
    assert_eq!(message.content, "Please measure the zeppelin latency first");
    let document = hits.iter().find(|hit| hit.id == DOCUMENT).unwrap();
    assert_eq!(document.kind, Kind::Document);
    assert_eq!(document.conversation_id, DOCUMENT);
    assert_eq!(document.author_kind, None);
    let literal = Literal::with_mode("zeppelin", all.match_mode).unwrap();
    let mut exact = reader.search_exact(&literal, 10, &all).unwrap();
    exact.sort_by(|left, right| left.id.cmp(&right.id));
    assert_eq!(ids(&exact), [DOCUMENT, MESSAGE, "native"]);
}

#[test]
fn provider_and_role_filters_name_native_history_and_source_names_connectors() {
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    project(&path);
    let reader = Storage::open_lexical(&path).unwrap();
    let search = |selection: Selection| {
        let mut found = reader
            .search_filtered("zeppelin", 10, None, None, Some(&scope(selection)))
            .unwrap()
            .into_iter()
            .map(|hit| hit.id)
            .collect::<Vec<_>>();
        found.sort_unstable();
        found
    };
    let codex = || vec![crate::provider::Provider::Codex];
    assert_eq!(
        search(Selection {
            provider: codex(),
            ..Selection::default()
        }),
        ["native"]
    );
    assert_eq!(
        search(Selection {
            role: vec![crate::role::Role::User],
            ..Selection::default()
        }),
        ["native"]
    );
    assert_eq!(
        search(Selection {
            source: vec!["github:org".into()],
            ..Selection::default()
        }),
        [DOCUMENT, MESSAGE]
    );
    assert_eq!(
        search(Selection {
            filter: Some(Filter::Not(Box::new(Filter::Provider(codex())))),
            ..Selection::default()
        }),
        [DOCUMENT, MESSAGE]
    );
    assert_eq!(
        search(Selection {
            conversation: vec![CHAT.into()],
            ..Selection::default()
        }),
        [MESSAGE]
    );
    // Time bounds (Unix seconds) apply to connector creation times in ms:
    // the document is at 500 ms, the message at exactly 1000 ms.
    assert_eq!(
        search(Selection {
            source: vec!["github:org".into()],
            before: Some(1),
            ..Selection::default()
        }),
        [DOCUMENT]
    );
    let legacy = reader
        .search_filtered(
            "zeppelin",
            10,
            Some(crate::provider::Provider::Codex),
            None,
            None,
        )
        .unwrap();
    assert_eq!(ids(&legacy), ["native"]);
}

#[test]
fn view_and_listing_resolve_connector_anchors() {
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    project(&path);
    let reader = Storage::open_lexical(&path).unwrap();
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
    let catalog = Storage::open_catalog(&path).unwrap();
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
fn edits_reindex_only_changed_units_and_deletions_leave_gold() {
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    project(&path);
    let generation = "test-generation";
    {
        let mut writer = Storage::open_writer(&path).unwrap();
        let pending = writer.pending_connector_embeddings(generation, 10).unwrap();
        assert_eq!(pending.len(), 3);
        assert!(
            pending
                .iter()
                .any(|(_, text)| text == "Runbook\nThe zeppelin dashboard lives here")
        );
        for (unit, _) in &pending {
            writer
                .store_connector_embedding(*unit, generation, &[1, 2, 3], 1.0)
                .unwrap();
        }
        assert!(writer.connector_embeddings_complete(generation).unwrap());
        writer.commit_writer().unwrap();
    }
    apply(
        &path,
        &batch(
            1,
            &serde_json::json!([
                {"op": "upsert-message", "id": "review-1", "chat_id": "pr-42",
                 "content": "Please measure the airship latency first",
                 "author": {"id": "alice", "kind": "human"}, "created_at": 1000},
                {"op": "delete-document", "id": "note"},
            ]),
        ),
    );
    assert!(Storage::open_lexical(&path).is_err());
    project(&path);
    let writer = Storage::open_writer(&path).unwrap();
    let pending = writer.pending_connector_embeddings(generation, 10).unwrap();
    assert_eq!(pending.len(), 1, "only the edited message needs a vector");
    assert!(pending[0].1.ends_with("airship latency first"));
    drop(writer);
    let reader = Storage::open_lexical(&path).unwrap();
    let all = scope(Selection::default());
    let zeppelin = reader
        .search_filtered("zeppelin", 10, None, None, Some(&all))
        .unwrap();
    assert_eq!(ids(&zeppelin), ["native"]);
    let airship = reader
        .search_filtered("airship", 10, None, None, Some(&all))
        .unwrap();
    assert_eq!(ids(&airship), [MESSAGE]);
}

#[test]
fn connector_vectors_join_native_dense_candidates_by_cosine() {
    let root = tempfile::tempdir().unwrap();
    let path = database(root.path());
    project(&path);
    let generation = "test-generation";
    {
        let mut writer = Storage::open_writer(&path).unwrap();
        for (unit, text) in writer.pending_connector_embeddings(generation, 10).unwrap() {
            // The message points along the query; everything else is orthogonal.
            let vector: [u8; 3] = if text.starts_with("Cache rollout\nPlease") {
                [127, 0, 0]
            } else {
                [0, 127, 0]
            };
            writer
                .store_connector_embedding(unit, generation, &vector, 127.0)
                .unwrap();
        }
        writer.commit_writer().unwrap();
    }
    let reader = Storage::open_lexical(&path).unwrap();
    let all = scope(Selection::default());
    let (hits, documents) = crate::semantic::local_candidates(
        &reader,
        generation,
        "unrelated",
        &[1.0, 0.0, 0.0],
        (None, None, crate::search_results::SearchUnit::Messages),
        Some(&all),
        crate::gold::config::Fusion::default(),
    )
    .unwrap();
    assert_eq!(hits[0].id, MESSAGE);
    assert_eq!(hits[0].source, Source::Connector("github:org".into()));
    assert!(hits[0].semantic_score.unwrap() > 0.99);
    assert_eq!(
        documents[0],
        "Cache rollout\nPlease measure the zeppelin latency first"
    );
    let (native_only, _) = crate::semantic::local_candidates(
        &reader,
        generation,
        "unrelated",
        &[1.0, 0.0, 0.0],
        (
            Some(crate::provider::Provider::Codex),
            None,
            crate::search_results::SearchUnit::Messages,
        ),
        Some(&all),
        crate::gold::config::Fusion::default(),
    )
    .unwrap();
    assert!(
        native_only
            .iter()
            .all(|hit| hit.source != Source::Connector("github:org".into()))
    );
}
