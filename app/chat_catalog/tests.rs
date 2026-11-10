use super::*;

#[test]
fn pagination_stops_at_the_documented_offset_cap() {
    let mut collector = Collector::new(Request {
        offset: 10_000,
        limit: 1,
        ..Default::default()
    })
    .unwrap();
    for number in 0..10002 {
        collector.push(row(&format!("id-{number:05}"), Some(number)));
    }
    let page = collector.finish();
    assert_eq!(page.chats.len(), 1);
    assert!(
        page.next_offset.is_none(),
        "never advertise an offset the request validator rejects"
    );
}

fn row(id: &str, time: Option<i64>) -> Chat {
    Chat {
        chat_id: id.into(),
        provider: Provider::Codex,
        node_id: "node".into(),
        metadata: ChatMetadata {
            enrichment: None,
            updated_at: time,
            ..Default::default()
        },
    }
}

#[test]
fn bounded_collection_deduplicates_better_copies_before_global_pagination() {
    let mut collector = Collector::new(Request {
        limit: 2,
        offset: 3,
        ..Default::default()
    })
    .unwrap();
    for number in 0..1000 {
        collector.push(row(&format!("id-{number:04}"), Some(number)));
        assert!(collector.rows.len() <= 6);
        assert_eq!(collector.rows.len(), collector.identities.len());
    }
    collector.push(row("id-0000", Some(1001)));
    collector.push(row("id-0999", Some(1003)));
    collector.push(row("id-0000", Some(0)));
    let page = collector.finish();
    assert_eq!(
        page.chats
            .into_iter()
            .map(|thread| thread.chat_id)
            .collect::<Vec<_>>(),
        ["id-0997", "id-0996"]
    );
    assert_eq!(page.next_offset, Some(5));
}

#[test]
fn metadata_filters_compose_and_time_bounds_exclude_unknown_activity() {
    let input = Request {
        provider: vec![Provider::Codex, Provider::ClaudeCode],
        title_contains: Some("%_LiTeRaL".into()),
        after: Some(2),
        before: Some(3),
        ..Default::default()
    };
    let mut collector = Collector::new(input.clone()).unwrap();
    for (id, time) in [
        ("unknown", None),
        ("before", Some(1999)),
        ("match", Some(2000)),
        ("at-end", Some(3000)),
    ] {
        let mut thread = row(id, time);
        thread.metadata.title = Some("100%_literal".into());
        collector.push(thread.clone());
        thread.provider = Provider::Opencode;
        collector.push(thread);
    }
    let page = collector.finish();
    assert_eq!(page.chats.len(), 1);
    assert_eq!(page.chats[0].chat_id, "match");
    assert!(input.admits_node("other", "caller"));
    let selected = Request {
        node: vec!["other".into()],
        exclude_self: true,
        ..Default::default()
    };
    assert!(selected.admits_node("other", "caller"));
    assert!(!selected.admits_node("caller", "caller"));
    assert!(!selected.admits_node("unselected", "caller"));
}

#[test]
fn thread_request_bounds_and_enums_reject_invalid_external_values() {
    for input in [
        Request {
            limit: 0,
            ..Default::default()
        },
        Request {
            limit: 101,
            ..Default::default()
        },
        Request {
            offset: 10001,
            ..Default::default()
        },
        Request {
            after: Some(-1),
            ..Default::default()
        },
        Request {
            title_contains: Some(String::new()),
            ..Default::default()
        },
        Request {
            node: vec!["bad\nnode".into()],
            ..Default::default()
        },
    ] {
        assert!(input.validate().is_err());
    }
    for json in [
        r#"{"sort":"random"}"#,
        r#"{"direction":"sideways"}"#,
        r#"{"extra":true}"#,
        r#"{"kind":["codex:chat"]}"#,
    ] {
        assert!(serde_json::from_str::<Request>(json).is_err());
    }
    Request::default().validate().unwrap();
}

#[test]
fn headline_keeps_the_first_non_empty_line_within_its_cap() {
    let mut title = "\n  \nFix the watcher\npassword=\"later-line\"\n".to_owned();
    headline(&mut title);
    assert_eq!(title, "Fix the watcher");
    let mut long = "é".repeat(5000);
    headline(&mut long);
    assert_eq!(long.chars().count(), 4096);
    for unchanged in ["", " \n ", "one line"] {
        let mut title = unchanged.to_owned();
        headline(&mut title);
        assert_eq!(title, unchanged);
    }
}
