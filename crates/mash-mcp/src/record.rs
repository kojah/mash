//! The `record` tool's input: one flat object.
//!
//! A record change is modelled as an action holding a body holding details,
//! and models, small ones above all, put fields at the wrong level of that:
//! `scope` beside the change, `recorded_by` inside the details. So the tool
//! takes every field at the top level and this module groups them into the
//! typed change, which validates them exactly as before.
use crate::records::{
    AssertionRelation, Citation, DecidedBy, DuplicatePolicy, Expected, Kind, Mutation, Review,
    Scope,
};
use mash_model::AppError;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    Create,
    Revise,
    Supersede,
    Confirm,
    Link,
    Forget,
}

#[derive(Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Proposed,
    Accepted,
    Active,
    Current,
    Superseded,
    Withdrawn,
    Retracted,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    /// A stable ID for this change, so a retry is safe.
    operation_id: String,
    /// create needs scope, kind, state, statement, `recorded_by` and evidence.
    /// revise (a correction) and supersede (the choice changed) need expected
    /// and the same record fields. confirm needs expected and new evidence.
    /// link needs expected, target and relation. forget needs expected.
    action: Action,
    /// Where a new record applies: {kind: personal}, {kind: repository,
    /// identity}, {kind: `local_project`, path} or {kind: chat, id}.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    scope: Option<Scope>,
    /// decision, preference or fact.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    kind: Option<Kind>,
    /// decision: proposed, accepted, superseded, withdrawn. preference:
    /// active, superseded, withdrawn. fact: current, superseded, retracted.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    state: Option<State>,
    /// The decision, preference or fact itself, in a sentence or two.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    statement: Option<String>,
    /// Why, when known.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    rationale: Option<String>,
    /// candidate (default) or established.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    review: Option<Review>,
    /// Who is recording, such as the agent's name. Attribution supplied by
    /// the caller, not authenticated human identity.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    recorded_by: Option<String>,
    /// The messages that support it: each a quote with `message_id`, or with
    /// `session_id` for your own session.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    evidence: Option<Vec<Citation>>,
    /// The record and revision being changed, for every action but create.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    expected: Option<Expected>,
    /// For link: the other record and its revision.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    target: Option<Expected>,
    /// For link. Your explicit reading; never inferred from similar wording.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    relation: Option<AssertionRelation>,
    /// For create: report (default) an identical live record, or reuse it.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    on_duplicate: Option<DuplicatePolicy>,
    /// For a decision: omit for the person's decision; agent for a choice the agent made.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    decided_by: Option<DecidedBy>,
    /// For a decision: the options that were not chosen.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    alternatives: Option<Vec<String>>,
    /// For a decision: what follows from it.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    consequences: Option<Vec<String>>,
    /// For a preference: when it applies.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    conditions: Option<Vec<String>>,
    /// For a fact: who reported it.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    reported_by: Option<String>,
    /// For a fact: when it stops holding, in Unix milliseconds.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    valid_until: Option<i64>,
    /// When the choice or claim applied, in Unix milliseconds, when known.
    #[serde(
        default,
        deserialize_with = "given",
        skip_serializing_if = "Option::is_none"
    )]
    effective_at: Option<i64>,
}

/// An optional field written as nothing, an empty string or an empty list is
/// one the caller did not give. Small models fill in every field they are
/// shown, with nothing in it, and a change means what it says either way.
fn given<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() || value == "" || value.as_array().is_some_and(Vec::is_empty) {
        return Ok(None);
    }
    T::deserialize(value)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// The fields of a record's body, and of its details within the body.
const BODY: [&str; 5] = [
    "statement",
    "rationale",
    "review",
    "recorded_by",
    "effective_at",
];
const DETAILS: [&str; 8] = [
    "kind",
    "state",
    "decided_by",
    "alternatives",
    "consequences",
    "conditions",
    "reported_by",
    "valid_until",
];

fn invalid(error: &serde_json::Error) -> AppError {
    AppError::usage(format!("record: {error}"))
}

impl Record {
    /// The typed change these fields describe. Which fields an action needs,
    /// and which a kind allows, is decided by the typed models.
    pub(crate) fn mutation(self) -> Result<Mutation, AppError> {
        let writes = matches!(
            self.action,
            Action::Create | Action::Revise | Action::Supersede
        );
        let Value::Object(mut change) = serde_json::to_value(&self).map_err(|e| invalid(&e))?
        else {
            return Err(AppError::usage("record: expected an object"));
        };
        let operation_id = change.remove("operation_id");
        let take = |change: &mut Map<String, Value>, names: &[&str]| -> Map<String, Value> {
            names
                .iter()
                .filter_map(|name| Some(((*name).to_owned(), change.remove(*name)?)))
                .collect()
        };
        let details = take(&mut change, &DETAILS);
        let mut body = take(&mut change, &BODY);
        if writes {
            body.insert("details".into(), Value::Object(details));
            change.insert("body".into(), Value::Object(body));
        } else if !body.is_empty() || !details.is_empty() {
            return Err(AppError::usage(
                "record: statement, kind, state and the other record fields apply only to create, revise and supersede",
            ));
        }
        serde_json::from_value(serde_json::json!({"operation_id": operation_id, "change": change}))
            .map_err(|error| invalid(&error))
    }

    /// The tool arguments that make `mutation`, for examples.
    pub(crate) fn arguments(mutation: &Mutation) -> Result<Value, serde_json::Error> {
        let Value::Object(mut nested) = serde_json::to_value(mutation)? else {
            return Ok(Value::Null);
        };
        let mut flat = Map::new();
        if let Some(operation_id) = nested.remove("operation_id") {
            flat.insert("operation_id".into(), operation_id);
        }
        let Some(Value::Object(mut change)) = nested.remove("change") else {
            return Ok(Value::Object(flat));
        };
        let body = change.remove("body");
        flat.extend(change);
        if let Some(Value::Object(mut body)) = body {
            let details = body.remove("details");
            flat.extend(body);
            if let Some(Value::Object(details)) = details {
                flat.extend(details);
            }
        }
        // What was left unsaid stays unsaid.
        flat.retain(|_, value| {
            !value.is_null() && !value.as_array().is_some_and(Vec::is_empty) && value != ""
        });
        Ok(Value::Object(flat))
    }
}

#[cfg(test)]
mod tests {
    use super::Record;
    use crate::records::{Change, Details, Mutation};
    use serde_json::json;

    fn change(arguments: serde_json::Value) -> Result<Mutation, mash_model::AppError> {
        serde_json::from_value::<Record>(arguments)
            .map_err(|error| mash_model::AppError::usage(error.to_string()))?
            .mutation()
    }

    #[test]
    fn flat_fields_make_the_typed_change_and_come_back_as_they_went_in() {
        let arguments = json!({
            "operation_id": "one", "action": "create",
            "scope": {"kind": "repository", "identity": "https://example.invalid/app.git"},
            "kind": "decision", "state": "accepted", "decided_by": "agent",
            "statement": "Use SSE", "rationale": "Simpler than sockets", "recorded_by": "agent",
            "evidence": [{"message_id": "m1", "quote": "use SSE", "purpose": "context"}]
        });
        let mutation = change(arguments.clone()).unwrap();
        let Change::Create { body, evidence, .. } = &mutation.change else {
            panic!("a create")
        };
        assert_eq!(body.statement, "Use SSE");
        assert_eq!(body.recorded_by, "agent");
        assert!(matches!(body.details, Details::Decision { .. }));
        assert_eq!(evidence[0].quote, "use SSE");
        let mut back = Record::arguments(&mutation).unwrap();
        // Defaults a caller may leave out are written out on the way back.
        back.as_object_mut().unwrap().remove("review");
        back["evidence"][0]
            .as_object_mut()
            .unwrap()
            .remove("expected_fingerprint");
        assert_eq!(back, arguments);

        let forget = change(json!({"operation_id": "two", "action": "forget", "expected": {"id": "r", "revision": 2}})).unwrap();
        assert!(matches!(forget.change, Change::Forget { .. }));
        assert_eq!(
            Record::arguments(&forget).unwrap(),
            json!({"operation_id": "two", "action": "forget", "expected": {"id": "r", "revision": 2}})
        );
    }

    #[test]
    fn what_an_action_or_a_kind_does_not_take_is_refused_by_name() {
        let message = |arguments| change(arguments).unwrap_err().error.message;
        let create = json!({"operation_id": "one", "action": "create", "kind": "decision", "state": "accepted",
            "statement": "Use SSE", "recorded_by": "agent", "evidence": []});
        assert!(message(create.clone()).contains("missing field `scope`"));
        let mut stateless = create.clone();
        stateless["scope"] = json!({"kind": "personal"});
        stateless.as_object_mut().unwrap().remove("kind");
        assert!(message(stateless).contains("kind"));
        // A state belongs to its kind, and a field to its kind's details.
        let mut wrong = create.clone();
        wrong["scope"] = json!({"kind": "personal"});
        wrong["state"] = json!("retracted");
        assert!(change(wrong.clone()).is_err());
        wrong["state"] = json!("accepted");
        wrong["conditions"] = json!(["at night"]);
        assert!(message(wrong).contains("conditions"));
        assert!(
            message(json!({"operation_id": "two", "action": "forget",
            "expected": {"id": "r", "revision": 2}, "statement": "x"}))
            .contains("apply only to create")
        );
        assert!(
            change(json!({"operation_id": "three", "action": "create", "change": {}})).is_err()
        );
    }

    #[test]
    fn a_field_given_as_nothing_is_a_field_not_given() {
        let mutation = change(json!({
            "operation_id": "one", "action": "create", "scope": {"kind": "personal"},
            "kind": "decision", "state": "accepted", "statement": "Use SSE", "recorded_by": "agent",
            "evidence": [{"message_id": "m1", "quote": "use SSE"}],
            "decided_by": "", "alternatives": [], "conditions": [], "reported_by": "",
            "effective_at": "", "valid_until": null, "expected": null, "rationale": ""
        }))
        .unwrap();
        assert!(matches!(mutation.change, Change::Create { .. }));
        // A value of the wrong kind is still refused.
        assert!(
            change(json!({"operation_id": "two", "action": "forget",
                "expected": {"id": "r", "revision": "two"}}))
            .is_err()
        );
    }
}
