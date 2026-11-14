//! The record tools' inputs: flat objects.
//!
//! A record is modelled as a body holding details, and a change as an action
//! holding that body. Models, small ones above all, put fields at the wrong
//! level of that: `scope` beside the change, `recorded_by` inside the
//! details. So the tools take a record's fields at the top level and this
//! module groups them into the typed models, which validate them exactly as
//! before.
use crate::records::{
    AssertionRelation, Citation, DecidedBy, DuplicatePolicy, Expected, Kind, Mutation, Review,
    Scope, extraction, matches,
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

/// A tool input with a record's own fields at the top level, after the
/// fields the tool adds.
macro_rules! with_record_fields {
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident: $kind:ty,)* }) => {
        $(#[$meta])*
        #[derive(Deserialize, Serialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct $name {
            $($(#[$field_meta])* $field: $kind,)*
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
    };
}

with_record_fields! {
    /// What the `record` tool takes.
    Record {
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
    }
}

with_record_fields! {
    /// One candidate record in an `extract_records` batch.
    Draft {
        /// The messages of this chat that support it: each a quote with `message_id`.
        #[serde(
            default,
            deserialize_with = "given",
            skip_serializing_if = "Option::is_none"
        )]
        evidence: Option<Vec<Citation>>,
        /// report (default) an identical live record, or reuse it.
        #[serde(
            default,
            deserialize_with = "given",
            skip_serializing_if = "Option::is_none"
        )]
        on_duplicate: Option<DuplicatePolicy>,
    }
}

with_record_fields! {
    /// What `find_record_matches` takes: the record about to be created.
    Find {
        /// Where the record would apply; matches are in this exact scope.
        scope: Scope,
    }
}

/// What `extract_records` takes.
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Extract {
    /// A stable ID for this batch, so a retry is safe.
    operation_id: String,
    /// Where the records apply.
    scope: Scope,
    /// The reviewed chat; every citation must be in it.
    chat_id: String,
    /// From `get_extraction_checkpoint`; a changed chat rejects the batch.
    source_fingerprint: String,
    /// The candidates, each with a record's fields at its own top level.
    /// Empty records a review with no conclusions.
    records: Vec<Draft>,
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

fn invalid(tool: &str, error: &serde_json::Error) -> AppError {
    AppError::usage(format!("{tool}: {error}"))
}

fn take(flat: &mut Map<String, Value>, names: &[&str]) -> Map<String, Value> {
    names
        .iter()
        .filter_map(|name| Some(((*name).to_owned(), flat.remove(*name)?)))
        .collect()
}

/// Whether any of a record's own fields are among `flat`.
fn names_a_record(flat: &Map<String, Value>) -> bool {
    BODY.iter()
        .chain(&DETAILS)
        .any(|name| flat.contains_key(*name))
}

/// Takes a record's fields out of `flat` as its body, details inside.
fn body(flat: &mut Map<String, Value>) -> Value {
    let details = take(flat, &DETAILS);
    let mut body = take(flat, &BODY);
    body.insert("details".into(), Value::Object(details));
    Value::Object(body)
}

fn object(tool: &str, input: &impl Serialize) -> Result<Map<String, Value>, AppError> {
    match serde_json::to_value(input).map_err(|error| invalid(tool, &error))? {
        Value::Object(fields) => Ok(fields),
        _ => Err(AppError::usage(format!("{tool}: expected an object"))),
    }
}

impl Record {
    /// The typed change these fields describe. Which fields an action needs,
    /// and which a kind allows, is decided by the typed models.
    pub(crate) fn mutation(self) -> Result<Mutation, AppError> {
        let action = self.action;
        let mut change = object("record", &self)?;
        let operation_id = change.remove("operation_id");
        if matches!(action, Action::Create | Action::Revise | Action::Supersede) {
            let body = body(&mut change);
            change.insert("body".into(), body);
        } else if names_a_record(&change) {
            return Err(AppError::usage(
                "record: statement, kind, state and the other record fields apply only to create, revise and supersede",
            ));
        }
        // No evidence given is an empty list of it, for the model to judge.
        if !matches!(action, Action::Link | Action::Forget) {
            change
                .entry("evidence")
                .or_insert_with(|| Value::Array(Vec::new()));
        }
        serde_json::from_value(serde_json::json!({"operation_id": operation_id, "change": change}))
            .map_err(|error| invalid("record", &error))
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

impl Extract {
    /// The typed batch, each candidate grouped into its body.
    pub(crate) fn request(self) -> Result<extraction::Request, AppError> {
        let tool = "extract_records";
        let mut batch = object(tool, &self)?;
        let drafts = match batch.remove("records") {
            Some(Value::Array(drafts)) => drafts,
            _ => Vec::new(),
        };
        let records: Vec<Value> = drafts
            .into_iter()
            .filter_map(|draft| match draft {
                Value::Object(mut draft) => {
                    let body = body(&mut draft);
                    draft.insert("body".into(), body);
                    draft
                        .entry("evidence")
                        .or_insert_with(|| Value::Array(Vec::new()));
                    Some(Value::Object(draft))
                }
                _ => None,
            })
            .collect();
        batch.insert("records".into(), Value::Array(records));
        serde_json::from_value(Value::Object(batch)).map_err(|error| invalid(tool, &error))
    }
}

impl Find {
    /// The typed request: the scope and the record's body.
    pub(crate) fn request(self) -> Result<matches::Request, AppError> {
        let tool = "find_record_matches";
        let mut request = object(tool, &self)?;
        let body = body(&mut request);
        request.insert("body".into(), body);
        serde_json::from_value(Value::Object(request)).map_err(|error| invalid(tool, &error))
    }
}

#[cfg(test)]
mod tests {
    use super::{Extract, Find, Record};
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

    #[test]
    fn a_batch_and_a_match_request_take_the_same_flat_record() {
        let record = json!({"kind": "preference", "state": "active", "statement": "Prefer SSE",
            "recorded_by": "agent", "conditions": ["for dashboards"]});
        let mut draft = record.clone();
        draft["evidence"] = json!([{"message_id": "m1", "quote": "prefer SSE"}]);
        let batch: Extract = serde_json::from_value(json!({
            "operation_id": "one", "scope": {"kind": "chat", "id": "c"}, "chat_id": "c",
            "source_fingerprint": "f", "records": [draft, record]
        }))
        .unwrap();
        let batch = batch.request().unwrap();
        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.records[0].body.statement, "Prefer SSE");
        assert_eq!(batch.records[0].evidence[0].quote, "prefer SSE");
        assert!(batch.records[1].evidence.is_empty());
        assert!(matches!(
            batch.records[0].body.details,
            Details::Preference { .. }
        ));

        let mut find = record.clone();
        find["scope"] = json!({"kind": "personal"});
        let find: Find = serde_json::from_value(find).unwrap();
        assert_eq!(find.request().unwrap().body.statement, "Prefer SSE");
        // The nested form is not this tool's input any more.
        assert!(
            serde_json::from_value::<Find>(json!({"scope": {"kind": "personal"}, "body": {}}))
                .is_err()
        );
    }
}
