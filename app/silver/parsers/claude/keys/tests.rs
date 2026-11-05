use super::*;

#[test]
fn inline_and_disk_preserve_missing_duplicate_unicode_and_first_positions() {
    let mut inline = Keys::with_limit(usize::MAX);
    let mut disk = Keys::with_limit(80);
    let mut expected = Vec::new();
    let mut first = BTreeMap::new();
    for index in 0..400 {
        let key = match index % 5 {
            0 => None,
            1 => Some("duplicate".to_owned()),
            2 => Some(format!("é🦀\0-{index}")),
            3 => Some(String::new()),
            _ => Some(format!("unique-{index}")),
        };
        if let Some(key) = &key {
            first.entry(key.clone()).or_insert(index);
        }
        expected.push(key.clone());
        inline.push(key.clone()).unwrap();
        disk.push(key).unwrap();
        assert_eq!(inline.len(), expected.len());
        assert_eq!(disk.len(), expected.len());
    }
    assert!(matches!(disk.state, State::Disk(_)));
    for (position, key) in expected.into_iter().enumerate() {
        assert_eq!(inline.get(position).unwrap(), key);
        assert_eq!(disk.get(position).unwrap(), key);
    }
    for (key, position) in first {
        assert_eq!(inline.first(&key).unwrap(), Some(position));
        assert_eq!(disk.first(&key).unwrap(), Some(position));
    }
    assert_eq!(inline.first("absent").unwrap(), None);
    assert_eq!(disk.first("absent").unwrap(), None);
    assert!(inline.get(400).is_err());
    assert!(disk.get(400).is_err());
}

#[test]
fn write_failure_poisoning_and_drop_remove_private_staging() {
    let mut keys = Keys::with_limit(0);
    keys.push(Some("private-source-id".to_owned())).unwrap();
    let State::Disk(store) = &keys.state else {
        panic!("expected spill")
    };
    let path = std::path::PathBuf::from(store.connection.path().unwrap());
    assert!(path.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    // Partial insert failure cannot leave an apparently successful ordinal ledger.
    store
        .connection
        .execute_batch("DROP TABLE first_positions")
        .unwrap();
    assert!(keys.push(Some("another".to_owned())).is_err());
    assert!(keys.get(0).is_err());
    assert!(keys.first("private-source-id").is_err());
    assert!(keys.push(None).is_err());
    assert!(!path.exists());

    let mut keys = Keys::with_limit(0);
    keys.push(None).unwrap();
    let State::Disk(store) = &keys.state else {
        panic!("expected spill")
    };
    let path = std::path::PathBuf::from(store.connection.path().unwrap());
    drop(keys);
    assert!(!path.exists());
}

#[test]
fn corrupted_lookup_and_ordered_identity_reads_fail_closed() {
    let mut keys = Keys::with_limit(0);
    keys.push(Some("uuid".to_owned())).unwrap();
    let State::Disk(store) = &keys.state else {
        panic!("expected spill")
    };
    store
        .connection
        .execute_batch(
            "UPDATE dialogue SET id=x'ff';
        UPDATE first_positions SET position=-1;",
        )
        .unwrap();
    assert!(keys.get(0).is_err());
    assert!(keys.first("uuid").is_err());
    store
        .connection
        .execute_batch("DROP TABLE dialogue; DROP TABLE first_positions;")
        .unwrap();
    assert!(keys.get(0).is_err());
    assert!(keys.first("uuid").is_err());
}

#[test]
fn native_dialogue_spill_keeps_forward_parents_duplicates_and_context() {
    use crate::silver::parsers::{parse_claude, stable_id};
    use std::io::Write;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native.jsonl");
    let mut output = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    let mut write = |uuid: &str, parent: Option<&str>, content: &str| {
        writeln!(
            output,
            "{}",
            serde_json::json!({
                "type": "user", "sessionId": "dialogue-thread", "uuid": uuid,
                "parentUuid": parent, "message": {"role": "user", "content": content},
            })
        )
        .unwrap();
    };
    let key = |position: usize| format!("dialogue-{position:036}");
    write("forward-child", Some(&key(0)), "forward reference");
    let mut expected = BTreeMap::new();
    let id = |uuid: &str| stable_id("claude-message", &["dialogue-thread", uuid]);
    expected.insert(
        id("forward-child"),
        ("forward reference".to_owned(), Some(id(&key(0)))),
    );
    for position in 0..12_000 {
        let uuid = key(position);
        let parent = (position > 0).then(|| key(position - 1));
        let content = format!("original dialogue {position}");
        write(&uuid, parent.as_deref(), &content);
        expected.insert(id(&uuid), (content, parent.as_deref().map(id)));
    }
    write(&key(0), None, "original dialogue 0");
    output.flush().unwrap();
    drop(output);
    let parsed = parse_claude(&path).unwrap();
    let chat = parsed.chat.unwrap();
    assert_eq!(chat.messages.len(), expected.len());
    let mut ordinals = BTreeMap::new();
    for message in chat.messages.into_rows().unwrap() {
        let message = message.unwrap();
        let (content, parent) = &expected[&message.id];
        assert_eq!(&message.content, content);
        assert_eq!(
            message.context.parent(),
            Some(parent.as_deref().unwrap_or(""))
        );
        ordinals.insert(message.id, message.ordinal);
    }
    assert_eq!(ordinals.len(), expected.len());
    assert_eq!(
        chat.claude_lineage.unwrap(),
        expected
            .into_iter()
            .map(|(id, (_, parent))| (id, parent))
            .collect()
    );
}

#[test]
#[ignore = "explicit metadata memory diagnostic; no models or full ingestion"]
fn dialogue_identity_memory_control() {
    use super::super::MessageIdentity;
    use crate::silver::parsers::stable_id;

    let count: usize = std::env::var("MASH_CLAUDE_KEYS_COUNT")
        .unwrap_or_else(|_| "300000".to_owned())
        .parse()
        .unwrap();
    assert!((1..=1_000_000).contains(&count));
    let control = std::env::var("MASH_CLAUDE_KEYS_CONTROL").unwrap();
    let key = |position: usize| {
        (!position.is_multiple_of(13))
            .then(|| format!("native-dialogue-{:036}", position % 250_000))
    };
    let mut digest = blake3::Hasher::new();
    let mut consume = |position: usize, key: Option<String>, identity: Option<MessageIdentity>| {
        digest.update(&(position as u64).to_le_bytes());
        match (key, identity) {
            (Some(key), Some(identity)) => {
                digest.update(key.as_bytes());
                digest.update(identity.id.as_bytes());
                digest.update(&(identity.position.unwrap() as u64).to_le_bytes());
            }
            (None, None) => {
                digest.update(b"missing");
            }
            _ => panic!("missing first-position lookup"),
        }
    };
    match control.as_str() {
        "reference" => {
            let values: Vec<_> = (0..count).map(key).collect();
            let mut first = BTreeMap::new();
            for (position, key) in values.iter().enumerate() {
                if let Some(key) = key {
                    first.entry(key.clone()).or_insert_with(|| MessageIdentity {
                        id: stable_id("claude-message", &["thread", key]),
                        position: Some(position),
                    });
                }
            }
            for (position, key) in values.iter().enumerate() {
                consume(
                    position,
                    key.clone(),
                    key.as_ref().map(|key| first[key].clone()),
                );
            }
        }
        "staged" => {
            let mut keys = Keys::default();
            for position in 0..count {
                keys.push(key(position)).unwrap();
            }
            assert!(matches!(keys.state, State::Disk(_)));
            for position in 0..count {
                let key = keys.get(position).unwrap();
                let identity = key.as_ref().map(|key| MessageIdentity {
                    id: stable_id("claude-message", &["thread", key]),
                    position: keys.first(key).unwrap(),
                });
                consume(position, key, identity);
            }
        }
        _ => panic!("reference or staged control required"),
    }
    println!(
        "control={control} count={count} digest={}",
        digest.finalize()
    );
}
