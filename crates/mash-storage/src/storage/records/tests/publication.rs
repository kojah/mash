use super::*;
use crate::storage::publication::JournalCoverage;

fn journal(storage: &Storage) -> Vec<(String, i64)> {
    storage
        .connection
        .prepare("SELECT record_id,revision FROM record_changes ORDER BY record_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn native_records_journal_prunes_only_acknowledged_coverage_and_keeps_newer_edits() {
    let (_directory, mut storage) = database();
    storage.prepare_publication_target("destination").unwrap();
    let first = storage
        .mutate_record(&create_request("first", decision()))
        .unwrap();
    let first_revision = journal(&storage)[0].1;
    let mut other = decision();
    other.statement = "Another choice".into();
    let second = storage
        .mutate_record(&create_request("second", other))
        .unwrap();
    let second_revision = journal(&storage)
        .into_iter()
        .find(|(id, _)| *id == second.record_id)
        .unwrap()
        .1;
    storage
        .mutate_record(&Mutation {
            operation_id: "correct-first".into(),
            change: Change::Revise {
                expected: expected(&first),
                body: decision(),
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap();
    let before = journal(&storage);
    let latest = before.iter().map(|(_, revision)| *revision).max().unwrap();
    storage
        .acknowledge_publication(
            "wrong-target",
            latest,
            JournalCoverage::Incremental { through: latest },
        )
        .unwrap();
    assert_eq!(journal(&storage), before);
    storage
        .acknowledge_publication("destination", latest, JournalCoverage::BaselinePending)
        .unwrap();
    assert_eq!(journal(&storage), before);
    storage
        .acknowledge_publication(
            "destination",
            first_revision,
            JournalCoverage::BaselineComplete {
                through: first_revision,
            },
        )
        .unwrap();
    assert_eq!(journal(&storage), before);
    storage
        .acknowledge_publication(
            "destination",
            second_revision,
            JournalCoverage::Incremental { through: latest },
        )
        .unwrap();
    assert_eq!(journal(&storage), vec![(first.record_id, latest)]);
    storage
        .acknowledge_publication(
            "destination",
            latest,
            JournalCoverage::BaselineComplete { through: latest },
        )
        .unwrap();
    assert_eq!(journal(&storage), []);
}

#[test]
fn native_records_journal_acknowledgement_failure_rolls_back_pruning() {
    let (directory, mut storage) = database();
    storage.prepare_publication_target("destination").unwrap();
    storage
        .mutate_record(&create_request("first", decision()))
        .unwrap();
    let before = journal(&storage);
    let revision = before[0].1;
    let acknowledged: i64 = storage
        .connection
        .query_row("SELECT acknowledged FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    storage.connection.execute_batch("CREATE TEMP TRIGGER interrupt_prune BEFORE UPDATE ON shared_change_state BEGIN SELECT RAISE(ABORT,'interrupted after journal pruning'); END;").unwrap();
    let error = storage
        .acknowledge_publication(
            "destination",
            revision,
            JournalCoverage::Incremental { through: revision },
        )
        .unwrap_err();
    assert!(
        error
            .error
            .message
            .contains("interrupted after journal pruning")
    );
    drop(storage);
    let storage = Storage::open(&directory.path().join("index.sqlite3")).unwrap();
    assert_eq!(journal(&storage), before);
    assert_eq!(
        storage
            .connection
            .query_row("SELECT acknowledged FROM shared_publication", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        acknowledged
    );
    storage
        .acknowledge_publication(
            "destination",
            revision,
            JournalCoverage::Incremental { through: revision },
        )
        .unwrap();
    assert_eq!(journal(&storage), []);
}
