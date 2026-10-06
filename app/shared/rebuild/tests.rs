use super::*;
use std::collections::BTreeMap;

fn row(thread: &str, ordinal: i64) -> Message {
    serde_json::from_value(serde_json::json!({
        "id": format!("{thread}-{ordinal}"),
        "conversation_id": thread,
        "ordinal": ordinal,
        "role": "user",
        "content": format!("canonical {ordinal}"),
        "projection": if ordinal % 4 == 0 {
            String::new()
        } else {
            format!("visible {ordinal}")
        },
        "created_at": null,
    }))
    .unwrap()
}

fn conversations() -> BTreeMap<String, protocol::ConversationRecord> {
    ["a", "b"]
        .into_iter()
        .map(|id| {
            (
                id.into(),
                protocol::ConversationRecord {
                    id: id.into(),
                    provider: crate::provider::Provider::Codex,
                    enrichment: None,
                    title: None,
                    project: None,
                    repository: None,
                    created_at: None,
                    updated_at: None,
                },
            )
        })
        .collect()
}

#[test]
fn retained_batches_preserve_thread_ordinal_and_hidden_row_boundaries() {
    let rows = (0..37)
        .map(|ordinal| row("a", ordinal))
        .chain((0..2).map(|ordinal| row("b", ordinal)))
        .map(|row| (row.id.clone(), row))
        .collect();
    let mut rows = ordered_rows(rows, &conversations())
        .unwrap()
        .into_iter()
        .peekable();
    let first = next_batch(&mut rows);
    let second = next_batch(&mut rows);
    let third = next_batch(&mut rows);
    assert_eq!(
        first.iter().map(|row| row.ordinal).collect::<Vec<_>>(),
        (0..32).collect::<Vec<_>>()
    );
    assert_eq!(
        second.iter().map(|row| row.ordinal).collect::<Vec<_>>(),
        (32..37).collect::<Vec<_>>()
    );
    assert_eq!(
        first
            .iter()
            .filter(|row| !row.projection.is_empty())
            .count(),
        24
    );
    assert_eq!(
        second
            .iter()
            .filter(|row| !row.projection.is_empty())
            .count(),
        3
    );
    assert!(
        first
            .iter()
            .chain(&second)
            .all(|row| row.conversation_id == "a")
    );
    assert_eq!(
        third.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["b-0", "b-1"]
    );
    assert!(next_batch(&mut rows).is_empty());
}

#[test]
fn retained_batch_preflight_rejects_bad_identity_or_missing_thread() {
    for (key, thread) in [("wrong", "a"), ("missing-0", "missing")] {
        let rows = [(key.into(), row(thread, 0))].into();
        assert!(ordered_rows(rows, &conversations()).is_err());
    }
}
