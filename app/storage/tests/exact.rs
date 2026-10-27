use super::*;
use crate::{search_filter::Selection, views::exact::Literal};

fn certify(writer: &mut Storage) {
    writer
        .begin_silver_scan(crate::provider::Provider::Codex)
        .unwrap();
    writer
        .finish_silver_scan(crate::provider::Provider::Codex, true)
        .unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.rebuild_derived_search_state().unwrap();
    writer.publish_lexical_manifest().unwrap();
}

#[test]
fn flexible_search_supports_prefixes_and_transpositions_on_read_only_views() {
    use crate::views::matching::{Kind, Mode};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("flexible.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    writer
        .replace_conversation(&conversation_with_messages(&[
            (
                "match",
                "Implement idempotency for kafka consumers using redis4",
            ),
            ("decoy", "redis40 has unrelated consumers"),
        ]))
        .unwrap();
    certify(&mut writer);
    writer.commit_writer().unwrap();
    drop(writer);
    let views = super::super::schema::views_path(&path);
    let before = std::fs::read(&views).unwrap();
    let reader = Storage::open_lexical(&path).unwrap();
    let scope = Selection::default().resolve("local").unwrap();
    assert_eq!(scope.match_mode, Mode::Flexible);
    for (query, kind) in [("idemp", Kind::Prefix), ("idempotnecy", Kind::Typo)] {
        let literal = Literal::with_mode(query, scope.match_mode).unwrap();
        let hits = reader.search_exact(&literal, 10, &scope).unwrap();
        assert_eq!(hits.len(), 1, "{query}");
        assert_eq!(hits[0].lexical_match.as_ref().unwrap().kind, kind);
        let hits = reader
            .search_filtered(query, 10, None, None, Some(&scope))
            .unwrap();
        assert_eq!(hits.len(), 1, "hybrid lexical {query}");
        assert_eq!(hits[0].lexical_match.as_ref().unwrap().kind, kind);
        assert!(
            reader
                .search_exact(
                    &Literal::with_mode(query, Mode::Strict).unwrap(),
                    10,
                    &scope
                )
                .unwrap()
                .is_empty()
        );
    }
    let hits = reader
        .search_exact(
            &Literal::with_mode("redis4", Mode::Flexible).unwrap(),
            10,
            &scope,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "match");
    assert!(
        reader
            .search_exact(
                &Literal::with_mode("\"idemp\"", Mode::Flexible).unwrap(),
                10,
                &scope
            )
            .unwrap()
            .is_empty()
    );
    let excluded = Selection {
        conversation: vec!["absent".into()],
        ..Default::default()
    }
    .resolve("local")
    .unwrap();
    assert!(
        reader
            .search_exact(
                &Literal::with_mode("idemp", Mode::Flexible).unwrap(),
                10,
                &excluded
            )
            .unwrap()
            .is_empty()
    );
    drop(reader);
    assert_eq!(before, std::fs::read(views).unwrap());
}

#[test]
fn filtered_flexible_vocabulary_preserves_chat_role_and_boolean_scope() {
    use crate::{role::Role, search_filter::Filter, views::matching::Mode};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("filtered-vocabulary.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    let mut selected = conversation_with_messages(&[
        ("selected", "idempotency idempotency idempotency"),
        (
            "assistant",
            "idempaaaa idempaaab idempaaac idempaaad idempaaae idempaaaf idempaaag idempaaah idempaaai",
        ),
    ]);
    selected.messages.inline_mut()[1].role = Role::Assistant;
    writer.replace_conversation(&selected).unwrap();
    let mut other = conversation_with_messages(&[(
        "outside",
        "idempbaaa idempbaab idempbaac idempbaad idempbaae idempbaaf idempbaag idempbaah idempbaai",
    )]);
    other.id = "other-chat".into();
    other.source = crate::silver::SourceIdentity::file("other.jsonl");
    writer.replace_conversation(&other).unwrap();
    certify(&mut writer);
    writer.commit_writer().unwrap();
    drop(writer);
    let reader = Storage::open_lexical(&path).unwrap();
    let scope = Selection {
        conversation: vec![selected.id],
        filter: Some(Filter::Not(Box::new(Filter::Role(vec![Role::Assistant])))),
        ..Default::default()
    }
    .resolve("local")
    .unwrap();
    for query in ["idemp", "idempotnecy"] {
        let plan = reader
            .lexical_plan(query, Mode::Flexible, Some(&scope))
            .unwrap();
        assert_eq!(
            plan.groups,
            vec![vec![query.to_owned(), "idempotency".to_owned()]]
        );
        let hits = reader
            .search_exact(
                &Literal::with_mode(query, Mode::Flexible).unwrap(),
                10,
                &scope,
            )
            .unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            ["selected"]
        );
        let hits = reader
            .search_filtered(query, 10, None, None, Some(&scope))
            .unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            ["selected"]
        );
    }
}

#[test]
fn lexical_publication_survives_forget_without_a_dense_manifest() {
    for dirty in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("lexical.sqlite3");
        let mut writer = Storage::open_writer(&path).unwrap();
        writer
            .replace_conversation(&conversation("delete this"))
            .unwrap();
        certify(&mut writer);
        if dirty {
            writer.mark_derived_search_dirty().unwrap();
        }
        writer.commit_writer().unwrap();
        assert!(writer.views_manifest().unwrap().is_none());
        drop(writer);
        let mut writer = Storage::open_existing(&path).unwrap();
        assert!(writer.forget(&conversation("delete this").id).unwrap());
        drop(writer);
        assert_eq!(Storage::open_lexical(&path).is_ok(), !dirty);
        if !dirty {
            let reader = Storage::open_lexical(&path).unwrap();
            assert_eq!(
                reader
                    .lexical_manifest()
                    .unwrap()
                    .unwrap()
                    .searchable_messages,
                0
            );
        }
    }
}

#[test]
fn exact_lexical_filters_literal_decoys_before_limit_and_never_reads_hidden_text() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("exact.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    let mut records = (0..1100)
        .map(|i| (format!("decoy-{i:04}"), "voyage code 4".to_owned()))
        .collect::<Vec<_>>();
    records.push((
        "literal-decoy".into(),
        "voyage-code-40 voyage-code-4-v2".into(),
    ));
    records.push(("match".into(), "use VOYAGE-code-4 today".into()));
    let refs = records
        .iter()
        .map(|(id, text)| (id.as_str(), text.as_str()))
        .collect::<Vec<_>>();
    writer
        .replace_conversation(&conversation_with_messages(&refs))
        .unwrap();
    let mut hidden = conversation_with_messages(&[("hidden-message", "voyage-code-4")]);
    hidden.id = "hidden-thread".into();
    hidden.source = crate::silver::SourceIdentity::file("hidden.jsonl");
    hidden.messages = vec![crate::silver::NormalizedMessage {
        attachments: Vec::new(),
        id: "hidden-message".into(),
        ordinal: 0,
        context: crate::message_context::MessageContext::Linear,
        role: crate::role::Role::Tool,
        content: "voyage-code-4".into(),
        dialogue_text: Some(String::new()),
        created_at: Some(0),
        tools: crate::tools::Activity::default(),
    }]
    .into();
    writer.replace_conversation(&hidden).unwrap();
    let mut legacy = conversation_with_messages(&[("legacy-tool", "voyage-code-4")]);
    legacy.id = "legacy-thread".into();
    legacy.source = crate::silver::SourceIdentity::file("legacy.jsonl");
    legacy.messages.inline_mut()[0].role = crate::role::Role::Tool;
    legacy.messages.inline_mut()[0].created_at = Some(i64::MAX);
    writer.replace_conversation(&legacy).unwrap();
    certify(&mut writer);
    writer.commit_writer().unwrap();
    drop(writer);
    // Dense embeddings are intentionally absent and their model is unavailable.
    let reader = Storage::open_lexical(&path).unwrap();
    assert_eq!(reader.counts().unwrap().embeddings, 0);
    let scope = Selection::default().resolve("local").unwrap();
    let literal = Literal::new("voyage-code-4").unwrap();
    let hits = reader.search_exact(&literal, 1, &scope).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "match");
    assert_eq!(hits[0].content, "use VOYAGE-code-4 today");
    let filtered = Selection {
        conversation: vec!["hidden-thread".into()],
        ..Default::default()
    }
    .resolve("local")
    .unwrap();
    assert!(
        reader
            .search_exact(&literal, 1, &filtered)
            .unwrap()
            .is_empty()
    );
    let filtered = Selection {
        node: vec!["other-node".into()],
        ..Default::default()
    }
    .resolve("local")
    .unwrap();
    assert!(
        reader
            .search_exact(&literal, 1, &filtered)
            .unwrap()
            .is_empty()
    );
    assert!(
        reader
            .connection
            .execute("DELETE FROM objects", [])
            .is_err()
    );
}

#[test]
fn lexical_readiness_requires_completed_current_silver_and_clean_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("exact.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    writer.commit_writer().unwrap();
    assert!(Storage::open_lexical(&path).is_err());
    drop(writer);
    let mut writer = Storage::open_writer(&path).unwrap();
    writer
        .replace_conversation(&conversation("voyage-code-4"))
        .unwrap();
    certify(&mut writer);
    writer.commit_writer().unwrap();
    assert!(Storage::open_lexical(&path).is_ok());
    drop(writer);
    let mut writer = Storage::open_writer(&path).unwrap();
    writer.mark_derived_search_dirty().unwrap();
    writer.commit_writer().unwrap();
    assert!(Storage::open_lexical(&path).is_err());
    drop(writer);
    let mut writer = Storage::open_writer(&path).unwrap();
    writer
        .replace_conversation(&conversation("changed history"))
        .unwrap();
    writer.rebuild_derived_search_state().unwrap();
    writer.commit_writer().unwrap();
    assert!(
        Storage::open_lexical(&path).is_err(),
        "old manifest must not certify changed silver"
    );
}
