//! Typed trace verification across actual HTTP publication failures.
use super::*;
use crate::shared::qdrant_index::publish::PublicationOutcome;
use tracing::{Instrument, instrument::WithSubscriber};

const TRACE: &str = "0123456789abcdef0123456789abcdef";

#[derive(Deserialize)]
struct Fields {
    message: String,
}
#[derive(Deserialize)]
struct Span {
    name: String,
    outcome: Option<PublicationOutcome>,
    retained: Option<bool>,
    trace_id: Option<String>,
}
#[derive(Deserialize)]
struct Event {
    fields: Fields,
    span: Option<Span>,
    #[serde(default)]
    spans: Vec<Span>,
}

pub(super) async fn commit(
    store: &Store,
    identity: &Identity,
    plan: crate::shared::snapshot::Plan,
    expected: Outcome,
) -> Result<crate::shared::protocol::Indexed, AppError> {
    let (bytes, dispatcher) = crate::observability::test_support::capture();
    let result = async {
        async {
            store
                .commit(identity, plan, "private-instance-canary".into(), true)
                .await
        }
        .instrument(tracing::info_span!("publication_request", trace_id = TRACE))
        .await
    }
    .with_subscriber(dispatcher)
    .await;
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(
        !output.contains("canary"),
        "publication arguments are private"
    );
    assert!(!output.contains("http://"), "backend URLs stay private");
    let events: Vec<Event> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let closed: Vec<&Event> = events
        .iter()
        .filter(|event| event.fields.message == "close")
        .collect();
    let publication = closed
        .iter()
        .find(|event| event.span.as_ref().unwrap().name == "qdrant_publish")
        .unwrap();
    let outcome = match expected {
        Outcome::AppliedFailure
        | Outcome::ActivatedDuringOptimization
        | Outcome::OptimizationFailed
        | Outcome::ReadinessFailed => PublicationOutcome::Observed,
        Outcome::DelayedFailure | Outcome::DelayedAcknowledgment => PublicationOutcome::Unresolved,
        Outcome::UnreadableVerification | Outcome::ActivatedDuringPublication => {
            PublicationOutcome::VerificationFailed
        }
        Outcome::CompletedLoser | Outcome::CleanupFailure => PublicationOutcome::Conflict,
        Outcome::StagingFailure => PublicationOutcome::StagingFailed,
    };
    assert_eq!(publication.span.as_ref().unwrap().outcome, Some(outcome));
    let names: BTreeMap<&str, &Event> = closed
        .iter()
        .filter_map(|event| event.span.as_ref().map(|span| (span.name.as_str(), *event)))
        .collect();
    assert!(names.contains_key("qdrant_stage_snapshot"));
    if !matches!(expected, Outcome::StagingFailure) {
        for name in [
            "qdrant_stage_catalog",
            "qdrant_stage_documents",
            "qdrant_publish_head",
            "qdrant_verify_head",
        ] {
            assert!(names.contains_key(name), "missing phase {name}");
        }
    }
    for (name, event) in &names {
        if name.starts_with("qdrant_") {
            assert!(
                event
                    .spans
                    .iter()
                    .any(|span| span.name == "publication_request"
                        && span.trace_id.as_deref() == Some(TRACE)),
                "phase {name} lacks request correlation"
            );
        }
    }
    if matches!(
        expected,
        Outcome::CompletedLoser | Outcome::StagingFailure | Outcome::CleanupFailure
    ) {
        assert_eq!(
            names["qdrant_cleanup"].span.as_ref().unwrap().retained,
            Some(matches!(expected, Outcome::CleanupFailure))
        );
    } else {
        assert!(
            !names.contains_key("qdrant_cleanup"),
            "ambiguous writes are retained"
        );
    }
    result
}
