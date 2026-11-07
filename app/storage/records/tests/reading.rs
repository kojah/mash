use super::*;

fn revise(storage: &mut Storage, receipt: &Receipt, operation: &str) -> Receipt {
    storage
        .mutate_record(&Mutation {
            operation_id: operation.into(),
            change: Change::Revise {
                expected: expected(receipt),
                body: decision(),
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap()
}

#[test]
fn native_records_history_cursor_survives_new_revisions_and_reaches_oldest() {
    let (_directory, mut storage) = database();
    let mut receipt = storage
        .mutate_record(&create_request("initial", decision()))
        .unwrap();
    for index in 0..10 {
        receipt = revise(&mut storage, &receipt, &format!("rev-{index}"));
    }
    let mut request = records::Get {
        id: receipt.record_id.clone(),
        history: true,
        limit: Some(3),
        ..Default::default()
    };
    let first = storage.record_page(&request).unwrap();
    assert_eq!(
        first
            .revisions
            .iter()
            .map(|item| item.revision)
            .collect::<Vec<_>>(),
        vec![11, 10, 9]
    );
    request.before_revision = first.next_before_revision;
    revise(&mut storage, &receipt, "arrived-during-pagination");
    let mut seen = vec![11, 10, 9];
    loop {
        let page = storage.record_page(&request).unwrap();
        seen.extend(page.revisions.iter().map(|item| item.revision));
        request.before_revision = page.next_before_revision;
        if request.before_revision.is_none() {
            break;
        }
    }
    assert_eq!(seen, (1..=11).rev().collect::<Vec<_>>());
    request.history = false;
    assert!(storage.record_page(&request).is_err());
    request.history = true;
    request.before_revision = Some(0);
    assert!(storage.record_page(&request).is_err());
}

#[test]
fn native_records_history_and_listing_byte_caps_keep_continuations() {
    let (_directory, mut storage) = database();
    let mut receipt = storage
        .mutate_record(&create_request("initial", decision()))
        .unwrap();
    for index in 0..9 {
        receipt = revise(&mut storage, &receipt, &format!("rev-{index}"));
    }
    // Maximum-size retained citations with distinct missing message identities:
    // stale evidence must still be inspectable, without loading source contents.
    storage
        .connection
        .execute("DELETE FROM record_evidence", [])
        .unwrap();
    storage.connection.execute("WITH RECURSIVE positions(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM positions WHERE n<255) INSERT INTO record_evidence SELECT r.record_id,r.revision,p.n,'chat','missing-'||p.n,?1,?2,'context','user' FROM record_revisions r CROSS JOIN positions p", params!["a".repeat(64), "x".repeat(8192)]).unwrap();
    let request = records::Get {
        id: receipt.record_id.clone(),
        history: true,
        limit: Some(20),
        ..Default::default()
    };
    let page = storage.record_page(&request).unwrap();
    assert!(encode(&page).unwrap().len() <= 16 * 1024 * 1024);
    assert!(page.revisions.len() < 10);
    assert!(page.next_before_revision.is_some());
    let next = storage
        .record_page(&records::Get {
            before_revision: page.next_before_revision,
            ..request
        })
        .unwrap();
    assert_eq!(page.revisions.len() + next.revisions.len(), 10);
    assert_eq!(next.next_before_revision, None);
    for index in 0..15 {
        let id = format!("copy-{index}");
        storage.connection.execute("INSERT INTO records SELECT ?1,owner,kind,scope,revision,forgotten FROM records WHERE id=?2", params![id, receipt.record_id]).unwrap();
        storage.connection.execute("INSERT INTO record_revisions SELECT ?1,revision,body,recorded_at FROM record_revisions WHERE record_id=?2 AND revision=10", params![id, receipt.record_id]).unwrap();
        storage.connection.execute("INSERT INTO record_evidence SELECT ?1,revision,position,chat_id,message_id,fingerprint,quote,purpose,source_role FROM record_evidence WHERE record_id=?2 AND revision=10", params![id, receipt.record_id]).unwrap();
    }
    let mut list = List {
        include_inactive: true,
        limit: Some(100),
        ..Default::default()
    };
    let first = storage.records(&list).unwrap();
    assert!(encode(&first).unwrap().len() <= 32 * 1024 * 1024);
    assert!(first.records.len() < 16);
    list.offset = first.next_offset.unwrap();
    let next = storage.records(&list).unwrap();
    let ids = first
        .records
        .iter()
        .chain(&next.records)
        .map(|item| item.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 16);
    assert_eq!(next.next_offset, None);
}

#[test]
fn native_records_links_are_pageable_without_losing_relationships() {
    let (_directory, mut storage) = database();
    let receipt = storage
        .mutate_record(&create_request("initial", decision()))
        .unwrap();
    for index in 0..130 {
        let id = format!("target-{index:03}");
        storage.connection.execute("INSERT INTO records SELECT ?1,owner,kind,scope,revision,forgotten FROM records WHERE id=?2", params![id, receipt.record_id]).unwrap();
        storage
            .connection
            .execute(
                "INSERT INTO record_links VALUES(?1,?2,'supports')",
                params![receipt.record_id, id],
            )
            .unwrap();
    }
    let mut request = records::Get {
        id: receipt.record_id,
        ..Default::default()
    };
    let first = storage.record_page(&request).unwrap();
    assert_eq!(first.links.len(), 128);
    request.link_offset = first.next_link_offset.unwrap();
    let next = storage.record_page(&request).unwrap();
    assert_eq!(next.links.len(), 2);
    assert_eq!(next.next_link_offset, None);
    assert_eq!(first.links[127].target_id, "target-127");
    assert_eq!(next.links[0].target_id, "target-128");
}
