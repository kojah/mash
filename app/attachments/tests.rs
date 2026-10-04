use super::*;

#[test]
fn retains_redacted_text_and_checks_integrity_without_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = Source::Base64 {
        name: "note.txt".into(),
        media_type: "text/plain".into(),
        data: STANDARD.encode("password=\"hunter2\"\nuseful notes"),
    };
    let record = retain(dir.path(), &Configuration::default(), "message", &[source])
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(record.availability, Availability::Retained);
    let text =
        String::from_utf8(STANDARD.decode(read(dir.path(), &record).unwrap()).unwrap()).unwrap();
    assert!(!text.contains("hunter2"));
    assert!(text.contains("useful notes"));
    fs::write(dir.path().join(record.digest.as_ref().unwrap()), "tampered").unwrap();
    assert!(read(dir.path(), &record).is_err());
}

#[test]
fn local_paths_require_allowed_roots_and_are_cached_offline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("files");
    fs::create_dir(&root).unwrap();
    let path = root.join("note.txt");
    fs::write(&path, "offline evidence").unwrap();
    let source = Source::File {
        name: "note.txt".into(),
        media_type: "text/plain".into(),
        path: path.clone(),
    };
    let cache = dir.path().join("cache");
    let denied = retain(
        &cache,
        &Configuration::default(),
        "message",
        std::slice::from_ref(&source),
    )
    .unwrap();
    assert_eq!(denied[0].availability, Availability::NotAllowed);
    let records = retain(
        &cache,
        &Configuration {
            share_files: false,
            allowed_roots: vec![root],
        },
        "message",
        &[source],
    )
    .unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(
        STANDARD.decode(read(&cache, &records[0]).unwrap()).unwrap(),
        b"offline evidence"
    );
}

#[test]
fn malformed_external_and_oversized_data_have_explicit_availability() {
    let dir = tempfile::tempdir().unwrap();
    let records = retain(
        dir.path(),
        &Configuration::default(),
        "message",
        &[
            Source::Base64 {
                name: "bad".into(),
                media_type: "image/png".into(),
                data: "bad base64!".into(),
            },
            url("remote", "https://example.invalid/private"),
            Source::Base64 {
                name: "big".into(),
                media_type: "image/png".into(),
                data: "A".repeat(MAX_BYTES.div_ceil(3) * 4 + 1),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        records.iter().map(|m| m.availability).collect::<Vec<_>>(),
        vec![
            Availability::Invalid,
            Availability::External,
            Availability::TooLarge
        ]
    );
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_allowed_attachment_roots() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("files");
    fs::create_dir(&root).unwrap();
    let outside = dir.path().join("secret");
    fs::write(&outside, "private").unwrap();
    let path = root.join("escape");
    std::os::unix::fs::symlink(outside, &path).unwrap();
    let record = retain(
        &dir.path().join("cache"),
        &Configuration {
            share_files: false,
            allowed_roots: vec![root],
        },
        "message",
        &[Source::File {
            name: "escape".into(),
            media_type: "text/plain".into(),
            path,
        }],
    )
    .unwrap();
    assert_eq!(record[0].availability, Availability::NotAllowed);
}
