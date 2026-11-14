use super::*;
use crate::records::{DuplicatePolicy, matches::Request};

#[test]
fn native_records_duplicate_reuse_is_explicit_and_does_not_confirm_or_publish() {
    let (_directory, mut storage) = database();
    let first = storage
        .mutate_record(&create_request("first", decision()))
        .unwrap();
    assert!(
        !encode(&create_request("legacy", decision()))
            .unwrap()
            .contains("on_duplicate")
    );
    let duplicate = storage
        .mutate_record(&create_request("repeat", decision()))
        .unwrap_err();
    assert!(duplicate.error.message.contains(&first.record_id));
    let publication: i64 = storage
        .connection
        .query_row("SELECT revision FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut request = create_request("reuse", decision());
    if let Change::Create {
        body,
        evidence,
        on_duplicate,
        ..
    } = &mut request.change
    {
        body.recorded_by = "another agent".into();
        *evidence = vec![citation("again", "Still choose Qdrant.")];
        *on_duplicate = DuplicatePolicy::Reuse;
    }
    let receipt = storage.mutate_record(&request).unwrap();
    assert!(receipt.reused);
    assert_eq!(receipt.record_id, first.record_id);
    assert_eq!(
        encode(&storage.mutate_record(&request).unwrap()).unwrap(),
        encode(&receipt).unwrap()
    );
    let record = storage.record(&first.record_id, true).unwrap();
    assert_eq!(record.revisions.len(), 1);
    assert_eq!(record.revisions[0].confirmations, 0);
    assert_eq!(record.revisions[0].evidence.len(), 2);
    assert_ne!(record.revisions[0].body.recorded_by, "another agent");
    assert_eq!(
        storage
            .connection
            .query_row("SELECT revision FROM shared_publication", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        publication
    );
    storage
        .mutate_record(&Mutation {
            operation_id: "erase".into(),
            change: Change::Forget {
                expected: expected(&first),
            },
        })
        .unwrap();
    storage.mutate_record(&request).unwrap();
    assert!(storage.record(&first.record_id, false).is_err());
}

#[test]
fn native_records_similar_suggestions_never_merge_or_resurrect_retired_choices() {
    let (_directory, mut storage) = database();
    let first = storage
        .mutate_record(&create_request("first", decision()))
        .unwrap();
    let request = Request {
        scope: Scope::Repository {
            identity: "repo".into(),
        },
        body: decision(),
    };
    let found = storage
        .record_matches(&request, crate::redaction::Policy::ON)
        .unwrap();
    assert_eq!(found.suggestions.len(), 1);
    assert!(found.suggestions[0].exact);
    let mut similar = decision();
    similar.statement = "Use Qdrant locally".into();
    let found = storage
        .record_matches(
            &Request {
                body: similar.clone(),
                ..request.clone()
            },
            crate::redaction::Policy::ON,
        )
        .unwrap();
    assert!(!found.suggestions[0].exact);
    assert_eq!(found.suggestions[0].overlap_percent, 66);
    let other = storage
        .mutate_record(&create_request("similar", similar))
        .unwrap();
    assert_ne!(other.record_id, first.record_id);
    let mut withdrawn = decision();
    withdrawn.details = Details::Decision {
        state: DecisionState::Withdrawn,
        decided_by: DecidedBy::User,
        alternatives: vec![],
        consequences: vec![],
    };
    storage
        .mutate_record(&Mutation {
            operation_id: "withdraw".into(),
            change: Change::Revise {
                expected: expected(&first),
                body: withdrawn,
                evidence: vec![citation("yes", "yes")],
            },
        })
        .unwrap();
    let fresh = storage
        .mutate_record(&create_request("return", decision()))
        .unwrap();
    assert_ne!(fresh.record_id, first.record_id);
    assert!(!fresh.reused);
    let other_scope = Request {
        scope: Scope::Personal,
        ..request
    };
    assert!(
        storage
            .record_matches(&other_scope, crate::redaction::Policy::ON)
            .unwrap()
            .suggestions
            .is_empty()
    );
}
