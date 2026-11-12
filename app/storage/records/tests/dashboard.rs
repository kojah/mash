use super::*;

#[test]
fn native_records_filters_precede_pagination_and_find_historical_chat_evidence() {
    let (_directory, mut storage) = database();
    let mut expected_ids = Vec::new();
    for index in 0..6 {
        let mut body = body(Details::Fact {
            state: FactState::Current,
            reported_by: "fixture".into(),
            valid_until: None,
        });
        body.statement = format!("Filter fixture {index}");
        if index % 2 == 0 {
            body.review = Review::Candidate;
        }
        let receipt = storage
            .mutate_record(&create_request(&format!("filter-{index}"), body))
            .unwrap();
        if index % 2 == 0 {
            expected_ids.push(receipt.record_id);
        }
    }
    storage
        .mutate_record(&create_request("decision", decision()))
        .unwrap();
    let mut input = List {
        kind: Some(records::Kind::Fact),
        state: Some(records::Lifecycle::Current),
        review: Some(Review::Candidate),
        query: Some("Filter fixture".into()),
        chat: Some("chat".into()),
        include_inactive: true,
        limit: Some(1),
        ..Default::default()
    };
    let mut actual = Vec::new();
    loop {
        let page = storage.records(&input).unwrap();
        assert_eq!(page.records.len(), 1);
        actual.push(page.records[0].id.clone());
        let Some(offset) = page.next_offset else {
            break;
        };
        input.offset = offset;
    }
    expected_ids.sort();
    actual.sort();
    assert_eq!(actual, expected_ids);
    input.offset = 0;
    input.state = Some(records::Lifecycle::Retracted);
    assert!(storage.records(&input).unwrap().records.is_empty());

    let mut fact = body(Details::Fact {
        state: FactState::Current,
        reported_by: "fixture".into(),
        valid_until: None,
    });
    fact.statement = "Historical citation fixture".into();
    let mut request = create_request("historical", fact.clone());
    if let Change::Create { scope, .. } = &mut request.change {
        *scope = Scope::Personal;
    }
    let receipt = storage.mutate_record(&request).unwrap();
    storage
        .mutate_record(&Mutation {
            operation_id: "correct-historical".into(),
            change: Change::Revise {
                expected: expected(&receipt),
                body: fact,
                evidence: vec![citation("other-message", "another project")],
            },
        })
        .unwrap();
    let page = storage
        .records(&List {
            chat: Some("chat".into()),
            query: Some("Historical citation fixture".into()),
            include_inactive: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].id, receipt.record_id);
    assert!(
        page.records[0].revisions[0]
            .evidence
            .iter()
            .all(|source| source.chat_id == "other")
    );
}
