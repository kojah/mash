use super::*;

fn fixtures() -> Vec<NormalizedMessage> {
    [None, Some(""), Some("visible projection")]
        .into_iter()
        .enumerate()
        .map(|(index, projection)| NormalizedMessage {
            attachments: Vec::new(),
            id: format!("message-{index}"),
            ordinal: i64::try_from(index).unwrap(),
            context: if index == 0 {
                crate::message_context::MessageContext::Root
            } else {
                crate::message_context::MessageContext::Parent { ordinal: 0 }
            },
            role: crate::role::Role::Other("custom-channel".to_owned()),
            content: format!("text {index} with control \0 and newline\n and escaped \\\""),
            dialogue_text: projection.map(str::to_owned),
            created_at: Some(i64::MAX - i64::try_from(index).unwrap()),
        })
        .collect()
}

fn staged() -> Messages {
    let mut builder = Builder {
        limit: 1,
        ..Builder::default()
    };
    for (index, message) in fixtures().into_iter().enumerate() {
        assert_eq!(builder.push(message.clone()).unwrap(), index);
        assert_eq!(builder.get(index).unwrap().unwrap().as_ref(), &message);
    }
    assert!(builder.get(99).unwrap().is_none());
    builder.finish().unwrap()
}

fn path(messages: &Messages) -> std::path::PathBuf {
    match messages {
        Messages::Staged(spool) => spool.path.to_path_buf(),
        Messages::Inline(_) => panic!("fixture must spill"),
    }
}

#[test]
fn staged_messages_have_independent_repeatable_reads_and_owned_cleanup() {
    let inline = Messages::stage(fixtures()).unwrap();
    assert!(matches!(inline, Messages::Inline(_)));
    let staged = staged();
    assert_eq!(staged.summary().unwrap(), inline.summary().unwrap());
    let path = path(&staged);
    let mut first = staged.rows().unwrap();
    let mut second = staged.rows().unwrap();
    assert_eq!(first.next().unwrap().unwrap().as_ref(), &fixtures()[0]);
    assert_eq!(second.next().unwrap().unwrap().as_ref(), &fixtures()[0]);
    assert_eq!(first.next().unwrap().unwrap().as_ref(), &fixtures()[1]);
    assert_eq!(second.next().unwrap().unwrap().as_ref(), &fixtures()[1]);
    drop((first, second));
    for _ in 0..2 {
        let decoded = staged
            .rows()
            .unwrap()
            .map(|row| row.unwrap().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(decoded, fixtures());
    }
    let mut owned = staged.into_rows().unwrap();
    assert!(path.exists(), "owned iterator must keep scratch data alive");
    assert_eq!(
        owned.by_ref().collect::<Result<Vec<_>, _>>().unwrap(),
        fixtures()
    );
    drop(owned);
    assert!(
        !path.exists(),
        "scratch data must be removed with its last owner"
    );
}

#[test]
fn truncated_corrupt_extra_and_missing_scratch_data_fail_explicitly() {
    for bytes in [b"".as_slice(), b"{broken", b"{}"] {
        let messages = staged();
        let path = path(&messages);
        std::fs::write(&path, bytes).unwrap();
        let mut rows = messages.rows().unwrap();
        assert!(rows.next().unwrap().is_err());
        assert!(rows.next().is_none(), "decoding errors fuse the iterator");
        drop(rows);
        drop(messages);
        assert!(!path.exists());
    }
    let messages = staged();
    let path = path(&messages);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    serde_json::to_writer(&mut file, &fixtures()[0]).unwrap();
    drop(file);
    let mut rows = messages.rows().unwrap();
    for _ in 0..3 {
        assert!(rows.next().unwrap().is_ok());
    }
    assert!(rows.next().unwrap().is_err());
    assert!(rows.next().is_none());
    drop(rows);
    std::fs::remove_file(&path).unwrap();
    assert!(messages.rows().is_err());
    assert!(messages.into_rows().is_err());
}

#[test]
fn failed_writes_and_flushes_never_finish_as_partial_messages() {
    for large in [false, true] {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        let path = temporary.path().to_owned();
        let readonly = File::open(&path).unwrap();
        let (writable, owned_path) = temporary.into_parts();
        drop(writable);
        let mut builder = Builder {
            state: State::Staged(Writer {
                output: BufWriter::new(readonly),
                path: owned_path,
                offsets: Vec::new(),
                position: 0,
                summary: summary::Summary::default(),
            }),
            limit: 1,
        };
        let mut message = fixtures().remove(0);
        if large {
            message.content = "x".repeat(32 * 1024);
        }
        let push = builder.push(message);
        if large {
            assert!(push.is_err());
        } else {
            assert!(push.is_ok());
        }
        assert!(builder.finish().is_err());
        assert!(!path.exists());
    }
}

#[test]
fn spilled_files_are_private_and_unknown_role_bytes_count_toward_the_budget() {
    let mut builder = Builder {
        limit: 128,
        ..Builder::default()
    };
    let mut message = fixtures().remove(0);
    message.content.clear();
    message.role = crate::role::Role::Other("large-role".repeat(128));
    builder.push(message).unwrap();
    let messages = builder.finish().unwrap();
    let path = path(&messages);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    drop(messages);
    assert!(!path.exists());
}
