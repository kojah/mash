use super::*;
use crate::{
    gold::{config::Build, exact::Literal, manifest::Manifest},
    search_filter::Selection,
};

fn certify(writer: &mut Storage) {
    writer
        .begin_silver_scan(crate::provider::Provider::Codex)
        .unwrap();
    writer
        .finish_silver_scan(crate::provider::Provider::Codex, true)
        .unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.rebuild_derived_search_state().unwrap();
    writer
        .publish_gold_manifest(&Manifest::new(
            Build::default(),
            "unavailable-model".into(),
            writer.silver_certificate().unwrap(),
        ))
        .unwrap();
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
        id: "hidden-message".into(),
        ordinal: 0,
        context: crate::message_context::MessageContext::Linear,
        role: crate::role::Role::Tool,
        content: "voyage-code-4".into(),
        dialogue_text: Some(String::new()),
        created_at: Some(0),
    }]
    .into();
    writer.replace_conversation(&hidden).unwrap();
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
            .execute("DELETE FROM messages", [])
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
