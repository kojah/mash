//! Generated operations exercise real SQLite against an independent reference.
use super::id;
use crate::connectors::silver::{Error, Store};
use crate::connectors::{Batch, Change, Object, Receipt, Role, Version};
use proptest::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
enum Operation {
    Put {
        source: u8,
        key: u8,
        content: String,
    },
    Delete {
        source: u8,
        key: u8,
    },
    Retry(u8),
    Conflict(u8),
    Stale(u8),
    RejectAtomic(u8),
    Reopen,
    Roundtrip,
}

fn operations() -> impl Strategy<Value = Operation> {
    prop_oneof![
        4 => (0u8..2, 0u8..3, "[a-z]{1,12}").prop_map(|(source, key, content)| Operation::Put { source, key, content }),
        2 => (0u8..2, 0u8..3).prop_map(|(source, key)| Operation::Delete { source, key }),
        2 => any::<u8>().prop_map(Operation::Retry),
        1 => any::<u8>().prop_map(Operation::Conflict),
        1 => (0u8..2).prop_map(Operation::Stale),
        1 => (0u8..2).prop_map(Operation::RejectAtomic),
        1 => Just(Operation::Reopen),
        1 => Just(Operation::Roundtrip),
    ]
}

fn object(key: u8, content: String) -> Object {
    Object {
        id: id(&format!("object-{key}")),
        role: Role::Document,
        kind: "notes:note".to_owned().try_into().unwrap(),
        parent: None,
        title: None,
        content: Some(content),
        author: None,
        reply_to: None,
        url: None,
        created_at: None,
        updated_at: None,
        metadata: None,
        attachments: Vec::new(),
    }
}

fn batch(source: u8, step: usize, revision: u64, changes: Vec<Change>) -> Batch {
    Batch {
        schema_version: Version,
        source_id: id(&format!("source-{source}")),
        batch_id: id(&format!("batch-{step}")),
        expected_revision: revision,
        changes,
        trigger: None,
        modes: None,
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one generated sequence exercises every transition in order"
)]
fn exercise(operations: &[Operation]) {
    let directory = tempfile::tempdir().unwrap();
    let mut path = directory.path().join("silver.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let mut revisions = [0u64; 2];
    let mut objects = BTreeMap::<(u8, u8), Object>::new();
    let mut receipts = Vec::<(Batch, Receipt)>::new();
    for (step, operation) in operations.iter().enumerate() {
        match operation {
            Operation::Put {
                source,
                key,
                content,
            } => {
                let value = object(*key, content.clone());
                let input = batch(
                    *source,
                    step,
                    revisions[usize::from(*source)],
                    vec![Change::UpsertObject(value.clone())],
                );
                let receipt = store.apply(&input).unwrap();
                revisions[usize::from(*source)] += 1;
                assert_eq!(receipt.source_revision, revisions[usize::from(*source)]);
                assert!(!receipt.views_ready);
                assert_eq!(receipt.source_id, input.source_id);
                assert_eq!(receipt.batch_id, input.batch_id);
                objects.insert((*source, *key), value);
                receipts.push((input, receipt));
            }
            Operation::Delete { source, key } => {
                let input = batch(
                    *source,
                    step,
                    revisions[usize::from(*source)],
                    vec![Change::DeleteObject {
                        id: object(*key, "unused".into()).id,
                    }],
                );
                let receipt = store.apply(&input).unwrap();
                revisions[usize::from(*source)] += 1;
                assert_eq!(receipt.source_revision, revisions[usize::from(*source)]);
                assert!(!receipt.views_ready);
                objects.remove(&(*source, *key));
                receipts.push((input, receipt));
            }
            Operation::Retry(choice) | Operation::Conflict(choice) if !receipts.is_empty() => {
                let (original, receipt) = &receipts[usize::from(*choice) % receipts.len()];
                if matches!(operation, Operation::Retry(_)) {
                    assert_eq!(store.apply(original).unwrap(), *receipt);
                } else {
                    let mut changed = original.clone();
                    // The fingerprint includes ordering, even with identical payload.
                    changed.expected_revision += 1;
                    assert_eq!(store.apply(&changed), Err(Error::BatchConflict));
                }
            }
            Operation::Stale(source) => {
                let revision = revisions[usize::from(*source)];
                let wrong = if revision == 0 { 1 } else { revision - 1 };
                let input = batch(
                    *source,
                    step,
                    wrong,
                    vec![Change::UpsertObject(object(0, "rejected".into()))],
                );
                assert_eq!(store.apply(&input), Err(Error::RevisionConflict));
            }
            Operation::RejectAtomic(source) => {
                let mut orphan = object(9, "orphan".into());
                orphan.role = Role::Entry;
                orphan.author = Some(crate::connectors::Author {
                    id: id("alice"),
                    name: None,
                    kind: crate::connectors::AuthorKind::Human,
                });
                orphan.kind = "chat:message".to_owned().try_into().unwrap();
                orphan.parent = Some(id("missing-parent"));
                let input = batch(
                    *source,
                    step,
                    revisions[usize::from(*source)],
                    vec![
                        Change::UpsertObject(object(0, "rolled-back".into())),
                        Change::UpsertObject(orphan),
                    ],
                );
                assert_eq!(store.apply(&input), Err(Error::InvalidReferences));
            }
            Operation::Reopen => {
                drop(store);
                store = Store::open(&path).unwrap();
            }
            Operation::Roundtrip => {
                drop(store);
                let archive = directory.path().join(format!("archive-{step}.jsonl"));
                crate::storage::export::write(&path, &archive).unwrap();
                let restored = directory.path().join(format!("restored-{step}.sqlite3"));
                crate::storage::export::restore(&archive, &restored, true).unwrap();
                path = restored;
                store = Store::open(&path).unwrap();
            }
            Operation::Retry(_) | Operation::Conflict(_) => {}
        }
        // Validate every source after every operation, including rejected writes
        // and retries. The model never derives its expected state from SQLite.
        assert_state(&store, &revisions, &objects, &receipts);
    }
}

fn assert_state(
    store: &Store,
    revisions: &[u64; 2],
    objects: &BTreeMap<(u8, u8), Object>,
    receipts: &[(Batch, Receipt)],
) {
    for source in 0u8..2 {
        for key in [0, 1, 2, 9] {
            let actual = store
                .object(
                    &id(&format!("source-{source}")),
                    &id(&format!("object-{key}")),
                )
                .unwrap();
            assert_eq!(actual.as_ref(), objects.get(&(source, key)));
        }
        let revision = super::super::sql::ConnectorRevision::builder()
            .id(&format!("source-{source}"))
            .build()
            .query_opt(&store.connection)
            .unwrap()
            .map_or(0, |row| row.revision.parse::<u64>().unwrap());
        assert_eq!(revision, revisions[usize::from(source)]);
    }
    // Old receipts must survive reopen, archive restoration and later edits.
    for (input, receipt) in receipts {
        let stored = super::super::sql::ConnectorReceipt::builder()
            .source_id(input.source_id.as_str())
            .batch_id(input.batch_id.as_str())
            .build()
            .query_one(&store.connection)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Receipt>(&stored.receipt).unwrap(),
            *receipt
        );
    }
}

fn config() -> ProptestConfig {
    let mut config = ProptestConfig::default();
    if std::env::var_os("PROPTEST_CASES").is_none() {
        config.cases = 32;
    }
    // Shrunk failing sequences are printed in nextest/CI logs. Keep
    // private run artifacts outside Git; promote regressions to named fixtures.
    config.failure_persistence = None;
    config.max_shrink_iters = 512;
    config
}

proptest! {
    #![proptest_config(config())]
    #[test]
    fn generated_connector_sequences_preserve_data_ordering_and_receipts(
        sequence in prop::collection::vec(operations(), 1..25)
    ) {
        exercise(&sequence);
    }
}

#[test]
fn connector_sequence_covers_retry_conflict_rollback_reopen_and_archive() {
    exercise(&[
        Operation::Put {
            source: 0,
            key: 0,
            content: "first".into(),
        },
        Operation::Put {
            source: 1,
            key: 0,
            content: "other-source".into(),
        },
        Operation::Put {
            source: 0,
            key: 0,
            content: "edited".into(),
        },
        Operation::Retry(0),
        Operation::Conflict(0),
        Operation::Stale(0),
        Operation::RejectAtomic(0),
        Operation::Reopen,
        Operation::Roundtrip,
        Operation::Retry(0),
        Operation::Delete { source: 0, key: 0 },
        Operation::Roundtrip,
        Operation::Retry(0),
    ]);
}
