use super::*;
use crate::records::transfer::Row;

fn shared_rows(storage: &Storage, after: Option<i64>) -> Result<Vec<Row>, AppError> {
    let mut rows = Vec::new();
    super::super::transfer::visit_shared(
        &storage.connection,
        after,
        crate::redaction::Policy::ON,
        |row| {
            rows.push(row);
            Ok(())
        },
    )?;
    Ok(rows)
}

#[test]
fn native_records_delta_does_not_decode_unselected_or_local_only_history() {
    let (_directory, mut storage) = database();
    let old = storage
        .mutate_record(&create_request("old", decision()))
        .unwrap();
    let base: i64 = storage
        .connection
        .query_row("SELECT revision FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut local = create_request("local", decision());
    if let Change::Create { scope, .. } = &mut local.change {
        *scope = Scope::LocalProject {
            path: "/project".into(),
        };
    }
    let local = storage.mutate_record(&local).unwrap();
    let mut changed_body = decision();
    changed_body.statement = "Another decision".into();
    let changed = storage
        .mutate_record(&create_request("changed", changed_body))
        .unwrap();
    let expected = shared_rows(&storage, Some(base)).unwrap();
    let ids = expected
        .iter()
        .filter_map(|row| match row {
            Row::Header { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![changed.record_id]);
    // If publication walks excluded history before filtering, either deliberately
    // malformed body will fail decoding and expose the unnecessary read.
    for id in [&old.record_id, &local.record_id] {
        storage
            .connection
            .execute(
                "UPDATE record_revisions SET body='not json' WHERE record_id=?1",
                [id],
            )
            .unwrap();
    }
    assert_eq!(
        encode(&shared_rows(&storage, Some(base)).unwrap()).unwrap(),
        encode(&expected).unwrap()
    );
    assert!(shared_rows(&storage, None).is_err());
}

#[test]
fn native_records_fingerprint_and_publication_never_decode_local_receipts() {
    let (_directory, mut storage) = database();
    storage
        .mutate_record(&create_request("first", decision()))
        .unwrap();
    let before = super::super::transfer::fingerprint(&storage.connection).unwrap();
    let shared = shared_rows(&storage, None).unwrap();
    let mut archive_hash = blake3::Hasher::new();
    super::super::transfer::visit(&storage.connection, |row| {
        if !matches!(
            row,
            Row::Operation { .. } | Row::ExtractionOperation { .. } | Row::Checkpoint { .. }
        ) {
            let bytes = serde_json::to_vec(&row).unwrap();
            archive_hash.update(&(bytes.len() as u64).to_le_bytes());
            archive_hash.update(&bytes);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(before, archive_hash.finalize().to_hex().as_str());
    storage.connection.execute_batch("UPDATE record_operations SET result='invalid local receipt'; INSERT INTO record_extractions VALUES('extraction','digest','invalid extraction receipt'); INSERT INTO record_checkpoints VALUES('invalid scope','chat','fingerprint');").unwrap();
    assert_eq!(
        super::super::transfer::fingerprint(&storage.connection).unwrap(),
        before
    );
    assert_eq!(
        encode(&shared_rows(&storage, None).unwrap()).unwrap(),
        encode(&shared).unwrap()
    );
    assert!(super::super::transfer::visit(&storage.connection, |_| Ok(())).is_err());
}
