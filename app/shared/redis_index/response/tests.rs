use super::*;
use crate::role::Role;

fn string(value: &str) -> Value {
    Value::BulkString(value.as_bytes().to_vec())
}
fn message_record() -> MessageRecord {
    MessageRecord {
        id: "message".into(),
        conversation_id: "chat".into(),
        ordinal: 2,
        context: crate::message_context::MessageContext::Linear,
        role: Role::User,
        content: "request\nprivate tool payload".into(),
        projection: "request".into(),
        created_at: None,
        generation: None,
        vector: None,
    }
}
fn record(message: &MessageRecord) -> Value {
    string(&serde_json::to_string(message).unwrap())
}
fn candidate_fields() -> Vec<(&'static str, Value)> {
    vec![
        (fields::RECORD, record(&message_record())),
        (fields::NODE_ID, string("node")),
        (fields::PROVIDER, string("codex")),
        (fields::PROJECTION, string("request")),
        (fields::DISTANCE, string("0.25")),
    ]
}
fn field_array(fields: Vec<(&str, Value)>) -> Value {
    Value::Array(
        fields
            .into_iter()
            .flat_map(|(name, value)| [string(name), value])
            .collect(),
    )
}
fn candidate(fields: Vec<(&str, Value)>, score: Value) -> Value {
    Value::Array(vec![
        Value::Int(1),
        string("key"),
        score,
        field_array(fields),
    ])
}
fn valid_candidate() -> Value {
    candidate(candidate_fields(), string("2.5"))
}
fn view_row(key: &str, message: &MessageRecord) -> [Value; 2] {
    [
        string(key),
        field_array(vec![(fields::RECORD, record(message))]),
    ]
}

#[test]
fn decodes_scores_projection_canonical_view_and_optional_metadata() {
    let reply = valid_candidate();
    let lexical = candidates(&reply, Ranking::Lexical).unwrap();
    assert_eq!(lexical[0].lexical_score, Some(-2.5));
    assert_eq!(lexical[0].semantic_score, None);
    assert_eq!(lexical[0].content, "request");
    assert_eq!(lexical[0].metadata.title, None);
    let semantic = candidates(&reply, Ranking::Semantic).unwrap();
    assert_eq!(semantic[0].lexical_score, None);
    assert_eq!(semantic[0].semantic_score, Some(0.75));
    let mut fields = candidate_fields();
    let conversation = ConversationRecord {
        enrichment: None,
        id: "chat".into(),
        provider: Provider::Codex,
        title: Some("title".into()),
        project: Some("project".into()),
        repository: None,
        created_at: Some(1),
        updated_at: Some(3),
    };
    fields.push((
        fields::CONVERSATION_RECORD,
        string(&serde_json::to_string(&conversation).unwrap()),
    ));
    let hit = candidates(&candidate(fields, Value::Double(2.5)), Ranking::Lexical).unwrap();
    assert_eq!(hit[0].metadata.title.as_deref(), Some("title"));
    assert_eq!(hit[0].metadata.project.as_deref(), Some("project"));
    let mut rows = vec![Value::Int(1)];
    rows.extend(view_row("key", &message_record()));
    assert_eq!(
        view(&Value::Array(rows)).unwrap()[0].content,
        "request\nprivate tool payload"
    );
}

#[test]
fn empty_results_and_paginated_totals_are_valid() {
    let empty = Value::Array(vec![Value::Int(0)]);
    assert!(view(&empty).unwrap().is_empty());
    for ranking in [Ranking::Lexical, Ranking::Semantic] {
        assert!(candidates(&empty, ranking).unwrap().is_empty());
        let Value::Array(mut paginated) = valid_candidate() else {
            unreachable!()
        };
        paginated[0] = Value::Int(100);
        assert_eq!(
            candidates(&Value::Array(paginated), ranking).unwrap().len(),
            1
        );
    }
}

#[test]
fn rejects_invalid_envelopes_counts_row_keys_and_duplicate_rows() {
    for reply in [
        Value::Nil,
        Value::Array(vec![]),
        Value::Array(vec![string("0")]),
        Value::Array(vec![Value::Int(-1)]),
        Value::Array(vec![Value::Int(1), string("key")]),
    ] {
        assert!(view(&reply).is_err());
        assert!(candidates(&reply, Ranking::Lexical).is_err());
    }
    for replacement in [Value::Int(0), Value::Int(-1), string("1")] {
        let Value::Array(mut values) = valid_candidate() else {
            unreachable!()
        };
        values[0] = replacement;
        assert!(candidates(&Value::Array(values), Ranking::Lexical).is_err());
    }
    for key in [
        Value::Nil,
        Value::Int(1),
        string(""),
        Value::BulkString(vec![255]),
    ] {
        let Value::Array(mut values) = valid_candidate() else {
            unreachable!()
        };
        values[1] = key;
        assert!(candidates(&Value::Array(values), Ranking::Lexical).is_err());
    }
    let Value::Array(mut values) = valid_candidate() else {
        unreachable!()
    };
    values[0] = Value::Int(2);
    values.extend_from_within(1..4);
    assert!(candidates(&Value::Array(values), Ranking::Lexical).is_err());
}

#[test]
fn rejects_missing_required_fields_but_accepts_additive_search_fields() {
    for name in [
        fields::RECORD,
        fields::NODE_ID,
        fields::PROVIDER,
        fields::PROJECTION,
    ] {
        let mut fields = candidate_fields();
        fields.retain(|(field, _)| *field != name);
        assert!(candidates(&candidate(fields, string("1")), Ranking::Lexical).is_err());
    }
    let mut fields = candidate_fields();
    fields.retain(|(name, _)| *name != fields::DISTANCE);
    let reply = candidate(fields, string("1"));
    assert!(candidates(&reply, Ranking::Lexical).is_ok());
    assert!(candidates(&reply, Ranking::Semantic).is_err());
    let mut fields = candidate_fields();
    fields.reverse();
    fields.push(("future_field", Value::BulkString(vec![255])));
    assert!(candidates(&candidate(fields, string("1")), Ranking::Semantic).is_ok());
}

#[test]
fn rejects_duplicate_malformed_and_mistyped_fields() {
    for duplicate in [fields::RECORD, fields::NODE_ID, "unknown"] {
        let mut fields = candidate_fields();
        fields.push((duplicate, string("x")));
        if duplicate == "unknown" {
            fields.push((duplicate, string("y")));
        }
        assert!(candidates(&candidate(fields, string("1")), Ranking::Lexical).is_err());
    }
    for fields in [
        Value::Nil,
        Value::Array(vec![string(fields::RECORD)]),
        Value::Array(vec![Value::Int(1), string("value")]),
    ] {
        assert!(
            candidates(
                &Value::Array(vec![Value::Int(1), string("key"), string("1"), fields]),
                Ranking::Lexical
            )
            .is_err()
        );
    }
    for field in [
        fields::RECORD,
        fields::NODE_ID,
        fields::PROVIDER,
        fields::PROJECTION,
        fields::DISTANCE,
    ] {
        let mut fields = candidate_fields();
        fields
            .iter_mut()
            .find(|(name, _)| *name == field)
            .unwrap()
            .1 = Value::Int(1);
        assert!(candidates(&candidate(fields, string("1")), Ranking::Semantic).is_err());
    }
}

#[test]
fn rejects_bad_scores_and_non_finite_distances() {
    for score in ["", "oops", "NaN", "inf", "-inf", "1e9999"] {
        assert!(
            candidates(
                &candidate(candidate_fields(), string(score)),
                Ranking::Lexical
            )
            .is_err()
        );
    }
    for score in [
        Value::Nil,
        Value::Int(1),
        Value::Double(f64::NAN),
        Value::Double(f64::INFINITY),
    ] {
        assert!(candidates(&candidate(candidate_fields(), score), Ranking::Semantic).is_err());
    }
    for distance in ["oops", "NaN", "inf", "-inf", "1e9999", "-1", "3"] {
        let mut fields = candidate_fields();
        fields
            .iter_mut()
            .find(|(name, _)| *name == fields::DISTANCE)
            .unwrap()
            .1 = string(distance);
        assert!(candidates(&candidate(fields, string("1")), Ranking::Semantic).is_err());
    }
}

#[test]
fn rejects_stored_schema_drift_and_inconsistent_identity_without_leaking_history() {
    for (field, value) in [
        (fields::RECORD, "{private credential}"),
        (fields::NODE_ID, ""),
        (fields::PROVIDER, "future-provider"),
        (fields::PROJECTION, "different projection"),
        (fields::CONVERSATION_RECORD, "{}"),
    ] {
        let mut fields = candidate_fields();
        if let Some((_, existing)) = fields.iter_mut().find(|(name, _)| *name == field) {
            *existing = string(value);
        } else {
            fields.push((field, string(value)));
        }
        let error = candidates(&candidate(fields, string("1")), Ranking::Lexical).unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
        assert!(!format!("{error:?}").contains("private credential"));
    }
    for provider in [Provider::ClaudeCode, Provider::Codex] {
        let conversation = ConversationRecord {
            enrichment: None,
            id: if provider == Provider::Codex {
                "other"
            } else {
                "chat"
            }
            .into(),
            provider,
            title: None,
            project: None,
            repository: None,
            created_at: None,
            updated_at: None,
        };
        let mut fields = candidate_fields();
        fields.push((
            fields::CONVERSATION_RECORD,
            string(&serde_json::to_string(&conversation).unwrap()),
        ));
        assert!(candidates(&candidate(fields, string("1")), Ranking::Lexical).is_err());
    }
    let mut json = serde_json::to_value(message_record()).unwrap();
    json["unexpected_schema_field"] = serde_json::json!(true);
    let mut fields = candidate_fields();
    fields[0].1 = string(&json.to_string());
    assert!(candidates(&candidate(fields, string("1")), Ranking::Lexical).is_err());
}

#[test]
fn view_validates_records_conversation_identity_and_order_without_losing_unknown_roles() {
    let mut first = message_record();
    first.id = "first".into();
    first.ordinal = 1;
    first.role = Role::Other("future-role".into());
    let second = message_record();
    let mut rows = vec![Value::Int(2)];
    rows.extend(view_row("second", &second));
    rows.extend(view_row("first", &first));
    let messages = view(&Value::Array(rows)).unwrap();
    assert_eq!(messages[0].id, "first");
    assert_eq!(messages[0].role, first.role);
    for bad in [
        MessageRecord {
            conversation_id: "other".into(),
            ..first.clone()
        },
        MessageRecord {
            ordinal: 2,
            ..first.clone()
        },
        MessageRecord {
            id: "message".into(),
            ..first.clone()
        },
        MessageRecord {
            ordinal: -1,
            ..first.clone()
        },
        MessageRecord {
            id: String::new(),
            ..first
        },
    ] {
        let mut rows = vec![Value::Int(2)];
        rows.extend(view_row("first", &bad));
        rows.extend(view_row("second", &second));
        assert!(view(&Value::Array(rows)).is_err());
    }
}
