use super::super::generated::ListChats;
use super::*;
use crate::provider::Provider;

fn catalog() -> (tempfile::TempDir, Storage) {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    for (id, provider, title, created, updated) in [
        ("a", "codex", Some("Alpha"), Some(1000), Some(3000)),
        ("b", "claude-code", Some("Beta"), Some(2000), Some(2000)),
        ("c", "codex", Some("Alpha"), Some(1000), Some(3000)),
        (
            "d",
            "opencode",
            Some("100%_literal"),
            Some(3000),
            Some(1000),
        ),
        ("e", "codex", None, None, None),
    ] {
        storage.connection.execute(
            "INSERT INTO conversations(id,provider,source_path,source_key,title,created_at,updated_at,project,repository) VALUES(?1,?2,?1,?1,?3,?4,?5,'/work/project','repo://project')",
            rusqlite::params![id, provider, title, created, updated],
        ).unwrap();
    }
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 0);
    storage
        .connection
        .pragma_update(None, "query_only", true)
        .unwrap();
    (directory, storage)
}

fn page(storage: &Storage, sort: &str, descending: bool, offset: i64, size: i64) -> Vec<String> {
    ListChats::builder()
        .all_providers(1)
        .providers_json("[]")
        .title_contains(None)
        .after_ms(None)
        .before_ms(None)
        .sort_key(sort)
        .descending(i64::from(descending))
        .page_offset(offset)
        .page_size(size)
        .build()
        .query_many(&storage.connection)
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect()
}

#[test]
fn generated_catalog_orders_missing_values_last_and_paginates_deterministically() {
    let (_directory, storage) = catalog();
    for (sort, descending, expected) in [
        ("updated_at", false, vec!["d", "b", "a", "c", "e"]),
        ("updated_at", true, vec!["a", "c", "b", "d", "e"]),
        ("created_at", false, vec!["a", "c", "b", "d", "e"]),
        ("created_at", true, vec!["d", "b", "a", "c", "e"]),
        ("title", false, vec!["d", "a", "c", "b", "e"]),
        ("title", true, vec!["b", "a", "c", "d", "e"]),
    ] {
        assert_eq!(page(&storage, sort, descending, 0, 10), expected);
        let mut paginated = Vec::new();
        for offset in [0, 2, 4] {
            paginated.extend(page(&storage, sort, descending, offset, 2));
        }
        assert_eq!(paginated, expected);
        assert_eq!(page(&storage, sort, descending, 5, 2), [] as [String; 0]);
    }
}

#[test]
fn generated_catalog_intersects_provider_title_and_activity_filters_before_paging() {
    let (_directory, storage) = catalog();
    let rows = ListChats::builder()
        .all_providers(0)
        .providers_json(r#"["codex","claude-code"]"#)
        .title_contains(Some("aLPha"))
        .after_ms(Some(3000))
        .before_ms(Some(4000))
        .sort_key("updated_at")
        .descending(1)
        .page_offset(1)
        .page_size(1)
        .build()
        .query_many(&storage.connection)
        .unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, "c");
    assert_eq!(row.provider, "codex");
    assert_eq!(row.title.as_deref(), Some("Alpha"));
    assert_eq!(row.project.as_deref(), Some("/work/project"));
    assert_eq!(row.repository.as_deref(), Some("repo://project"));
    assert_eq!(row.created_at, Some(1000));
    assert_eq!(row.updated_at, Some(3000));
    let dated = ListChats::builder()
        .all_providers(1)
        .providers_json("[]")
        .title_contains(None)
        .after_ms(Some(0))
        .before_ms(None)
        .sort_key("updated_at")
        .descending(1)
        .page_offset(0)
        .page_size(10)
        .build()
        .query_many(&storage.connection)
        .unwrap();
    assert_eq!(
        dated.len(),
        4,
        "unknown activity does not match a time bound"
    );
    assert!(dated.iter().all(|row| row.updated_at.is_some()));
}

#[test]
fn generated_catalog_title_matching_is_literal_and_provider_lists_are_exact() {
    let (_directory, storage) = catalog();
    for (providers, title, after, before, expected) in [
        (r#"["opencode"]"#, "%_", None, None, vec!["d"]),
        (r#"["codex"]"#, "%_", None, None, vec![]),
        (r#"["cod"]"#, "Alpha", None, None, vec![]),
        (r#"["codex"]"#, "' OR TRUE --", None, None, vec![]),
        ("[]", "Alpha", None, None, vec![]),
        (r#"["codex"]"#, "Alpha", Some(3000), Some(3000), vec![]),
    ] {
        let rows = ListChats::builder()
            .all_providers(0)
            .providers_json(providers)
            .title_contains(Some(title))
            .after_ms(after)
            .before_ms(before)
            .sort_key("title")
            .descending(0)
            .page_offset(0)
            .page_size(10)
            .build()
            .query_many(&storage.connection)
            .unwrap();
        assert_eq!(
            rows.into_iter().map(|row| row.id).collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn typed_catalog_adapter_preserves_generated_order_filters_and_bounded_windows() {
    use crate::chat_catalog::{Collector, Direction, Request, Sort};
    let (_directory, storage) = catalog();
    for (sort, key) in [
        (Sort::UpdatedAt, "updated_at"),
        (Sort::CreatedAt, "created_at"),
        (Sort::Title, "title"),
    ] {
        for direction in [Direction::Asc, Direction::Desc] {
            let input = Request {
                sort,
                direction,
                offset: 1,
                limit: 2,
                ..Default::default()
            };
            let rows = storage.chat_candidates(&input, "laptop").unwrap();
            assert_eq!(rows.len(), 4);
            assert!(rows.iter().all(|row| row.node_id == "laptop"));
            let mut collector = Collector::new(input).unwrap();
            for row in rows {
                collector.push(row);
            }
            let actual = collector.finish();
            assert_eq!(actual.next_offset, Some(3));
            assert_eq!(
                actual
                    .chats
                    .iter()
                    .map(|row| row.conversation_id.clone())
                    .collect::<Vec<_>>(),
                page(&storage, key, matches!(direction, Direction::Desc), 1, 2)
            );
        }
    }
    let request = Request {
        provider: vec![Provider::Codex, Provider::ClaudeCode],
        title_contains: Some("aLPha".into()),
        after: Some(3),
        before: Some(4),
        node: vec!["laptop".into()],
        ..Default::default()
    };
    let rows = storage.chat_candidates(&request, "laptop").unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.conversation_id.as_str())
            .collect::<Vec<_>>(),
        ["a", "c"]
    );
    for request in [
        Request {
            exclude_self: true,
            ..request.clone()
        },
        Request {
            node: vec!["other".into()],
            ..request
        },
    ] {
        assert!(
            storage
                .chat_candidates(&request, "laptop")
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn catalog_opens_only_existing_current_schemas_without_migration_or_writes() {
    let (directory, storage) = catalog();
    let path = directory.path().join("index.sqlite3");
    let reader = Storage::open_catalog(&path).unwrap();
    assert!(
        reader
            .connection
            .execute("DELETE FROM conversations", [])
            .is_err()
    );
    assert_eq!(reader.counts().unwrap().conversations, 5);
    drop(reader);
    let absent = directory.path().join("missing.sqlite3");
    assert!(Storage::open_catalog(&absent).is_err());
    assert!(!absent.exists());
    storage
        .connection
        .pragma_update(None, "query_only", false)
        .unwrap();
    for version in [SCHEMA_VERSION - 1, SCHEMA_VERSION + 1] {
        storage
            .connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        assert!(Storage::open_catalog(&path).is_err());
        assert_eq!(
            storage
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            version
        );
    }
}
