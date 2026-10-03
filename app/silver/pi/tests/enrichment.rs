use super::*;
use crate::silver::enrichment::{Evidence, Origin, SourceField, VERSION};
use rusqlite::Connection;

fn inferred() -> Origin {
    Origin::FirstVisibleUserMessage {
        message_id: super::super::super::parsers::stable_id(
            "message",
            &["pi", "pi-session", "visible"],
        ),
        rule_version: VERSION,
    }
}

fn native(record: &str) -> Origin {
    Origin::SourceRecord {
        field: SourceField::SessionInfoName,
        record_id: super::super::super::parsers::stable_id(
            "source-record",
            &[
                "pi",
                &super::super::super::parsers::stable_id("session", &["pi", "pi-session"]),
                record,
            ],
        ),
        rule_version: VERSION,
    }
}

fn history(path: &Path) -> Vec<u8> {
    let parsed = parse(path).unwrap().conversation.unwrap();
    serde_json::to_vec(
        &parsed
            .messages
            .rows()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn title_source_updates_preserve_dialogue_even_when_native_and_inferred_texts_are_equal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            header(),
            message(
                "hidden",
                None,
                "user",
                "<environment_context>hidden</environment_context>"
            ),
            message("visible", Some("hidden"), "user", "visible request")
        ),
    )
    .unwrap();
    let before = history(&path);
    let database = directory.path().join("silver.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    let mut previous = storage.silver_certificate().unwrap().revision;
    for (record, expected) in [
        (None, inferred()),
        (
            Some(entry(
                "name-one",
                Some("visible"),
                json!({"type":"session_info","name":" visible request "}),
            )),
            native("name-one"),
        ),
        (
            Some(entry(
                "name-two",
                Some("name-one"),
                json!({"type":"session_info","name":"visible request"}),
            )),
            native("name-two"),
        ),
        (
            Some(entry(
                "clear",
                Some("name-two"),
                json!({"type":"session_info","name":null}),
            )),
            inferred(),
        ),
    ] {
        if let Some(record) = record {
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap(),
                "{record}"
            )
            .unwrap();
        }
        let parsed = parse(&path).unwrap().conversation.unwrap();
        assert_eq!(parsed.title.as_deref(), Some("visible request"));
        assert_eq!(
            Evidence::capture(&parsed).unwrap().title,
            Some(expected.clone())
        );
        assert_eq!(history(&path), before);
        index(&mut storage, &options(&path)).unwrap();
        assert_eq!(
            storage
                .conversation_enrichment(&parsed.id)
                .unwrap()
                .unwrap()
                .title,
            Some(expected)
        );
        let revision = storage.silver_certificate().unwrap().revision;
        assert!(
            revision > previous,
            "changed origin must advance silver even for equal title text"
        );
        previous = revision;
        assert_eq!(storage.counts().unwrap().messages, 2);
        assert_eq!(
            index(&mut storage, &options(&path))
                .unwrap()
                .checkpoint_skipped_sources,
            1
        );
    }
}

#[test]
fn unknown_title_origins_replay_without_changing_the_source_or_inventing_evidence_on_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    fixture(&path);
    let parsed = parse(&path).unwrap().conversation.unwrap();
    let expected = Evidence::capture(&parsed).unwrap();
    let database = directory.path().join("silver.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let mut old = expected.clone();
    old.title = Some(Origin::Unknown);
    Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE conversation_enrichment SET evidence=?1",
            [serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
    let stored = Storage::open_catalog(&database).unwrap();
    assert_eq!(
        stored.conversation_enrichment(&parsed.id).unwrap(),
        Some(old.clone())
    );
    stored
        .with_publication_snapshot(|snapshot| {
            snapshot.visit(crate::shared::protocol::BatchKind::Conversations, |batch| {
                let crate::shared::protocol::Batch::Conversations { records } = batch else {
                    panic!("conversation batch")
                };
                assert_eq!(records[0].enrichment.as_ref(), Some(&old));
                Ok(())
            })
        })
        .unwrap();
    let stamp = source_stamp_for(&SourceIdentity::file(&path)).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(
        !storage
            .source_checkpoint_matches(Provider::Pi, &SourceIdentity::file(&path), &stamp)
            .unwrap()
    );
    let replay = index(&mut storage, &options(&path)).unwrap();
    assert_eq!(replay.changed_messages, 0);
    assert_eq!(replay.checkpoint_skipped_sources, 0);
    assert_eq!(
        source_stamp_for(&SourceIdentity::file(&path)).unwrap(),
        stamp
    );
    storage.commit_writer().unwrap();
    drop(storage);
    let stored = Storage::open_catalog(&database).unwrap();
    assert_eq!(
        stored.conversation_enrichment(&parsed.id).unwrap(),
        Some(expected.clone())
    );
    assert_eq!(
        stored
            .thread_candidates(&crate::thread_catalog::Request::default(), "local")
            .unwrap()[0]
            .metadata
            .enrichment
            .as_ref(),
        Some(&expected)
    );
}
