//! Decisions an agent makes itself, beside the person's own.
use super::*;

fn agent_decision(review: Review) -> Body {
    Body {
        statement: "Keep search in its own module".into(),
        review,
        ..body(Details::Decision {
            state: DecisionState::Accepted,
            decided_by: DecidedBy::Agent,
            alternatives: vec![],
            consequences: vec![],
        })
    }
}

fn reported(message: &str, quote: &str) -> Citation {
    Citation {
        purpose: EvidenceUse::Report,
        ..citation(message, quote)
    }
}

fn create(operation: &str, body: Body, evidence: Vec<Citation>) -> Mutation {
    Mutation {
        operation_id: operation.into(),
        change: Change::Create {
            scope: Scope::Repository {
                identity: "repo".into(),
            },
            body,
            evidence,
            on_duplicate: records::DuplicatePolicy::Report,
        },
    }
}

#[test]
fn agent_decision_is_settled_by_the_agents_own_message_without_endorsement() {
    let (_directory, mut storage) = database();
    // The person's words alone do not show the agent made this choice.
    let error = storage
        .mutate_record(&create(
            "person-only",
            agent_decision(Review::Established),
            vec![reported("again", "Still choose Qdrant")],
        ))
        .unwrap_err();
    assert!(error.error.message.contains("agent's own message"));
    let receipt = storage
        .mutate_record(&create(
            "agent",
            agent_decision(Review::Established),
            vec![reported("proposal", "Choose Qdrant")],
        ))
        .unwrap();
    let record = storage.record(&receipt.record_id, true).unwrap();
    let revision = &record.revisions[0];
    assert!(revision.body.details.agent_decided());
    assert_eq!(revision.body.review, Review::Established);
    assert_eq!(
        serde_json::to_value(&revision.body.details).unwrap()["decided_by"],
        "agent"
    );
    // The person's decision is stored as before, without the field.
    let person = serde_json::to_value(decision().details).unwrap();
    assert!(person.get("decided_by").is_none());
    // A candidate needs no such citation.
    storage
        .mutate_record(&create(
            "candidate",
            Body {
                statement: "Split the indexer".into(),
                ..agent_decision(Review::Candidate)
            },
            vec![reported("again", "Still choose Qdrant")],
        ))
        .unwrap();
}

#[test]
fn agent_decision_cannot_replace_the_persons_but_the_person_can_replace_the_agents() {
    let (_directory, mut storage) = database();
    let person = storage
        .mutate_record(&create_request("person", decision()))
        .unwrap();
    for change in [
        Change::Supersede {
            expected: expected(&person),
            body: agent_decision(Review::Established),
            evidence: vec![reported("proposal", "Choose Qdrant")],
        },
        Change::Revise {
            expected: expected(&person),
            body: agent_decision(Review::Established),
            evidence: vec![reported("proposal", "Choose Qdrant")],
        },
    ] {
        let error = storage
            .mutate_record(&Mutation {
                operation_id: "agent-over-person".into(),
                change,
            })
            .unwrap_err();
        assert!(error.error.message.contains("cannot replace the person"));
    }
    let agent = storage
        .mutate_record(&create(
            "agent",
            agent_decision(Review::Established),
            vec![reported("proposal", "Choose Qdrant")],
        ))
        .unwrap();
    let mut theirs = decision();
    theirs.statement = "Keep search inline".into();
    storage
        .mutate_record(&Mutation {
            operation_id: "person-over-agent".into(),
            change: Change::Supersede {
                expected: expected(&agent),
                body: theirs,
                evidence: vec![citation("again", "Still choose Qdrant")],
            },
        })
        .unwrap();
}
