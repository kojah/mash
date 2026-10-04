use super::*;
use std::io::{Cursor, Write};

fn event(kind: &str, body: &str) -> String {
    format!(
        r#"{{"timestamp":"2026-09-30T00:00:00Z","type":"event_msg","payload":{{"type":"item_completed","item":{{"stdout":"{body}","type":"{kind}","id":"tool-1"}}}}}}"#
    )
}

#[test]
fn oversized_compaction_replay_preserves_canonical_messages_and_line_numbers() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("replay.jsonl");
    let first = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"canonical before"}]}}"#;
    let last = first.replace("canonical before", "canonical after");
    let replay = format!(
        r#"{{"timestamp":"2026-08-08T00:00:00Z","ordinal":5727,"type":"compacted","payload":{{"message":"summary","replacement_history":[{{"content":"{}"}}],"window_number":1}}}}"#,
        "x".repeat(super::super::super::MAX_RECORD_BYTES + 1)
    );
    let input = format!("{first}\n{replay}\r\n{last}\n");
    std::fs::write(&path, &input).unwrap();
    let parsed = crate::silver::parsers::parse_codex(&path)
        .unwrap()
        .conversation
        .unwrap();
    let messages = parsed.messages.inline();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "canonical before");
    assert_eq!(messages[1].content, "canonical after");
    let numbered = Lines::new(BufReader::new(Cursor::new(input)))
        .numbered()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(numbered, vec![(1, first.to_owned()), (3, last)]);
    for invalid in [
        replay.replace(
            "\"replacement_history\":[",
            "\"replacement_history\":\"bad\",\"other\":[",
        ),
        replay.replace("\"type\":\"compacted\"", "\"type\":\"unknown\""),
        replay[..replay.len() - 1].to_owned(),
    ] {
        assert!(
            Lines::new(BufReader::new(Cursor::new(invalid)))
                .next()
                .unwrap()
                .is_err()
        );
    }
}

#[test]
fn oversized_tool_mirrors_are_validated_and_do_not_consume_following_messages() {
    for kind in ["FileChange", "CommandExecution"] {
        let large = event(kind, &"x".repeat(super::super::super::MAX_RECORD_BYTES + 1));
        let large = large.replace("\"timestamp\":", "\"ordinal\":8508,\"timestamp\":");
        for ending in ["\n", "\r\n"] {
            let mut rows = Lines::new(BufReader::new(Cursor::new(format!(
                "before{ending}{large}{ending}after"
            ))));
            assert_eq!(rows.next().unwrap().unwrap(), "before");
            assert_eq!(rows.next().unwrap().unwrap(), "after");
            assert!(rows.next().is_none());
        }
        let mut rows = Lines::new(BufReader::new(Cursor::new(large)));
        assert!(rows.next().is_none());
    }
}

#[test]
fn numbered_codex_records_include_streamed_events_and_blank_lines() {
    let large = event(
        "FileChange",
        &"x".repeat(super::super::super::MAX_RECORD_BYTES + 1),
    );
    let input = format!("\n{large}\r\nnot-json\n");
    let mut rows = Lines::new(BufReader::new(Cursor::new(input))).numbered();
    assert_eq!(rows.next().unwrap().unwrap(), (1, String::new()));
    assert_eq!(rows.next().unwrap().unwrap(), (3, "not-json".to_owned()));
    assert!(rows.next().is_none());
}

#[test]
fn oversized_dialogue_unknown_types_and_malformed_envelopes_fail_closed() {
    let body = "x".repeat(super::super::super::MAX_RECORD_BYTES + 1);
    let valid = event("FileChange", &body);
    let invalid = [
        event("UserMessage", &body),
        event("AgentMessage", &body),
        event("FutureItem", &body),
        valid.replace("event_msg", "response_item"),
        valid.replace("item_completed", "item_started"),
        format!("{valid} trailing"),
        valid[..valid.len() - 1].to_owned(),
        valid.replace(
            "\"timestamp\":",
            "\"cwd\":\"/must-not-discard-metadata\",\"timestamp\":",
        ),
    ];
    for input in invalid {
        let mut rows = Lines::new(BufReader::new(Cursor::new(input)));
        assert!(rows.next().unwrap().is_err());
        assert!(rows.next().is_none());
    }
}

#[test]
fn ignored_strings_still_require_valid_utf8_across_reader_boundaries() {
    let input = event("FileChange", "中文é😀");
    for capacity in 1..8 {
        assert!(skip_event(&mut BufReader::with_capacity(capacity, Cursor::new(&input))).unwrap());
    }
    let mut invalid = input.into_bytes();
    let index = invalid.iter().position(|byte| *byte >= 128).unwrap();
    invalid[index] = 0xff;
    assert!(skip_event(&mut BufReader::new(Cursor::new(invalid))).is_err());
}

#[test]
fn streaming_validator_bounds_bytes_and_buffer_retention() {
    for bytes in [MAX_EVENT_BYTES, MAX_EVENT_BYTES + 1] {
        let mut source = BufReader::new(std::io::repeat(b'x').take(bytes as u64));
        let mut line = Line {
            reader: &mut source,
            bytes: 0,
            ended: false,
            pending: Vec::new(),
        };
        let result = std::io::copy(&mut line, &mut std::io::sink());
        assert_eq!(result.is_ok(), bytes == MAX_EVENT_BYTES);
        assert!(line.bytes <= MAX_EVENT_BYTES);
        assert!(line.pending.capacity() <= 16 * 1024);
    }
}

#[test]
fn real_parser_and_index_preserve_history_across_large_lifecycle_envelopes() {
    use crate::silver::{IndexOptions, index};
    use crate::{provider::Provider, storage::Storage};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("codex.jsonl");
    let message = r#"{"timestamp":"2026-09-30T00:00:00Z","type":"response_item","payload":{"type":"message","id":"user-1","role":"user","content":[{"type":"input_text","text":"cbioportal canonical history"}]}}"#;
    std::fs::write(&path, format!("{message}\n")).unwrap();
    let options = IndexOptions {
        providers: [(Provider::Codex, vec![path.clone()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let database = root.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    let large = event(
        "FileChange",
        &"x".repeat(super::super::super::MAX_RECORD_BYTES + 1),
    );
    writeln!(file, "{large}").unwrap();
    let valid_length = file.metadata().unwrap().len();
    let parsed = crate::silver::parsers::parse_codex(&path)
        .unwrap()
        .conversation
        .unwrap();
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(
        parsed.messages.inline()[0].content,
        "cbioportal canonical history"
    );
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    let source = crate::silver::SourceIdentity::file(&path);
    let checkpoint = crate::silver::source_stamp_for(&source).unwrap();
    drop(storage);
    // Malformed large mirrors must not replace the previous indexed snapshot.
    file.write_all(&large.as_bytes()[..large.len() - 1])
        .unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(index(&mut storage, &options).is_err());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Codex, &source, &checkpoint)
            .unwrap()
    );
    assert_eq!(
        storage.search("cbioportal", 10, None, None).unwrap().len(),
        1
    );
    file.set_len(valid_length).unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert!(
        storage
            .source_checkpoint_matches(
                Provider::Codex,
                &source,
                &crate::silver::source_stamp_for(&source).unwrap()
            )
            .unwrap()
    );
}
