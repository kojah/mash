//! The input schemas as tools publish them.
//!
//! A model reads every tool's schema in every session, so each one is reduced
//! to what tells a model how to call the tool. Generated schemas also say
//! things only a validator uses: that no other properties are allowed, the
//! machine width of a number, that an omitted value defaults to nothing, and
//! "this or null" for every optional argument. Arguments are still checked
//! against the full typed models when a tool is called.
use serde_json::{Map, Value};

/// `schema` with the validator-only parts removed, alternatives told apart
/// by a constant made one object, and every definition written where it is
/// used. A model follows a schema it can read top to bottom better than one
/// that sends it elsewhere for each part, and after flattening, writing them
/// out is no longer than sharing them.
pub(super) fn plain(schema: &Map<String, Value>) -> Map<String, Value> {
    let stripped = strip(Value::Object(schema.clone()));
    let Value::Object(mut schema) = flatten(stripped.clone()) else {
        return schema.clone();
    };
    // Each round writes out the definitions that refer to no other, which
    // leaves those that named them referring to none.
    for _ in 0..8 {
        let leaves: Map<String, Value> = match schema.get("$defs") {
            Some(Value::Object(definitions)) => definitions
                .iter()
                .filter(|(_, definition)| !definition.to_string().contains("\"#/$defs/"))
                .map(|(name, definition)| (name.clone(), definition.clone()))
                .collect(),
            _ => break,
        };
        if leaves.is_empty() {
            break;
        }
        let Value::Object(mut next) = inline(Value::Object(schema), &leaves) else {
            unreachable!("inlining keeps an object an object")
        };
        if let Some(Value::Object(definitions)) = next.get_mut("$defs") {
            definitions.retain(|name, _| !leaves.contains_key(name));
        }
        schema = next;
    }
    let text = Value::Object(schema.clone()).to_string();
    if let Some(Value::Object(definitions)) = schema.get_mut("$defs") {
        definitions.retain(|name, _| text.contains(&reference(name)));
        if definitions.is_empty() {
            schema.remove("$defs");
        }
    }
    // Written out, alternatives that were only references can be told apart.
    if let Value::Object(flat) = flatten(Value::Object(schema.clone())) {
        schema = flat;
    }
    // Definitions that name themselves or each other stay shared; if any
    // reference lost its target, publish the schema as it was.
    if dangling(&schema) {
        return match stripped {
            Value::Object(stripped) => stripped,
            _ => schema,
        };
    }
    schema
}

/// The one property that names an alternative, when every alternative is an
/// object that sets it to its own constant.
fn tag(alternatives: &[Value]) -> Option<String> {
    let constants = |alternative: &Value| -> Vec<String> {
        alternative
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(_, property)| property.get("const").is_some_and(Value::is_string))
            .map(|(name, _)| name.clone())
            .collect()
    };
    let first = constants(alternatives.first()?);
    let [name] = &first[..] else { return None };
    alternatives
        .iter()
        .all(|alternative| constants(alternative) == first)
        .then(|| name.clone())
}

/// Alternatives told apart by one named constant become one object: the name
/// takes any of the constants, the other properties are all listed once, and
/// the description says which of them each alternative needs. A model fills
/// in one flat object far more reliably than it picks among several shapes,
/// and some model services refuse `oneOf`. Alternatives that give one
/// property different meanings are left as they are.
fn flatten(value: Value) -> Value {
    let mut fields = match value {
        Value::Array(items) => return Value::Array(items.into_iter().map(flatten).collect()),
        Value::Object(fields) => fields
            .into_iter()
            .map(|(name, value)| (name, flatten(value)))
            .collect::<Map<String, Value>>(),
        other => return other,
    };
    let Some(Value::Array(alternatives)) = fields.get("oneOf") else {
        return Value::Object(fields);
    };
    if let Some(values) = alternatives.iter().map(named).collect::<Option<Vec<_>>>() {
        return Value::Object(choice(fields, values));
    }
    let Some(tag) = tag(alternatives) else {
        return Value::Object(fields);
    };
    let mut properties = Map::new();
    let mut names = Vec::new();
    let mut needs = Vec::new();
    let mut varying: Vec<String> = Vec::new();
    for alternative in alternatives {
        let Some(own) = alternative.get("properties").and_then(Value::as_object) else {
            return Value::Object(fields);
        };
        let name = own[&tag]["const"].as_str().unwrap_or_default().to_owned();
        for (property, schema) in own.iter().filter(|(property, _)| **property != tag) {
            match properties.get_mut(property) {
                None => {
                    properties.insert(property.clone(), schema.clone());
                }
                Some(seen) if seen == schema => {}
                // One name with its own values in each alternative, such as a
                // state: every value is listed, and which belong to which is said.
                Some(seen) => {
                    let (Some(mut all), Some(more)) = (named(seen), named(schema)) else {
                        return Value::Object(fields);
                    };
                    for value in more {
                        if !all.contains(&value) {
                            all.push(value);
                        }
                    }
                    *seen = serde_json::json!({"type": "string", "enum": all});
                    if !varying.contains(property) {
                        varying.push(property.clone());
                    }
                }
            }
        }
        let required: Vec<&str> = alternative
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|property| *property != tag)
            .collect();
        if !required.is_empty() {
            needs.push(format!("{name} needs {}.", required.join(", ")));
        }
        names.push(Value::String(name));
    }
    for property in &varying {
        for alternative in alternatives {
            let own = &alternative["properties"];
            if let (Some(name), Some(values)) = (own[&tag]["const"].as_str(), named(&own[property]))
            {
                let values: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
                needs.push(format!("{name} {property}: {}.", values.join(", ")));
            }
        }
    }
    let mut all = Map::new();
    all.insert(
        tag.clone(),
        serde_json::json!({"type": "string", "enum": names}),
    );
    all.extend(properties);
    fields.remove("oneOf");
    fields.insert("type".into(), "object".into());
    fields.insert("properties".into(), Value::Object(all));
    fields.insert("required".into(), serde_json::json!([tag]));
    describe(&mut fields, &needs.join(" "));
    Value::Object(fields)
}

/// Alternatives that are each a named value or a few become one list of
/// values, with what the described ones mean.
fn choice(mut fields: Map<String, Value>, values: Vec<Vec<Value>>) -> Map<String, Value> {
    let meanings: Vec<String> = fields
        .get("oneOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|one| {
            let meaning = one.get("description")?.as_str()?;
            Some(format!(
                "{}: {}",
                one.get("const")?.as_str()?,
                meaning.replace('\n', " ")
            ))
        })
        .collect();
    fields.remove("oneOf");
    fields.insert("type".into(), "string".into());
    fields.insert(
        "enum".into(),
        Value::Array(values.into_iter().flatten().collect()),
    );
    describe(&mut fields, &meanings.join(" "));
    fields
}

/// The values of a schema that is only a choice among named strings.
fn named(schema: &Value) -> Option<Vec<Value>> {
    let fields = schema.as_object()?;
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), "type" | "enum" | "const" | "description"))
    {
        return None;
    }
    match (fields.get("const"), fields.get("enum")) {
        (Some(value @ Value::String(_)), None) => Some(vec![value.clone()]),
        (None, Some(Value::Array(values))) if values.iter().all(Value::is_string) => {
            Some(values.clone())
        }
        _ => None,
    }
}

/// Adds `more` to what a schema says about itself.
fn describe(fields: &mut Map<String, Value>, more: &str) {
    if more.is_empty() {
        return;
    }
    let text = match fields.get("description").and_then(Value::as_str) {
        Some(said) => format!("{said} {more}"),
        None => more.to_owned(),
    };
    fields.insert("description".into(), Value::String(text));
}

/// Whether anything refers to a definition that is no longer there.
fn dangling(schema: &Map<String, Value>) -> bool {
    fn names<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
        match value {
            Value::Array(items) => items.iter().for_each(|item| names(item, found)),
            Value::Object(fields) => {
                if let Some(name) = fields
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|path| path.strip_prefix("#/$defs/"))
                {
                    found.push(name);
                }
                fields.values().for_each(|field| names(field, found));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    schema.values().for_each(|value| names(value, &mut found));
    let definitions = schema.get("$defs").and_then(Value::as_object);
    found
        .iter()
        .any(|name| definitions.is_none_or(|definitions| !definitions.contains_key(*name)))
}

fn reference(name: &str) -> String {
    format!("\"#/$defs/{name}\"")
}

fn null(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("null")
}

fn strip(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(strip).collect()),
        Value::Object(fields) => {
            let mut fields: Map<String, Value> = fields
                .into_iter()
                .map(|(name, value)| (name, strip(value)))
                .collect();
            fields.remove("$schema");
            fields.remove("format");
            if fields.get("additionalProperties") == Some(&Value::Bool(false)) {
                fields.remove("additionalProperties");
            }
            if fields.get("default").is_some_and(|default| {
                default.is_null()
                    || default == &Value::Bool(false)
                    || default.as_array().is_some_and(Vec::is_empty)
            }) {
                fields.remove("default");
            }
            // "integer or null" is how an optional argument is generated.
            if let Some(Value::Array(types)) = fields.get("type") {
                let named: Vec<&Value> = types.iter().filter(|kind| *kind != "null").collect();
                if let [only] = named[..] {
                    fields.insert("type".into(), only.clone());
                }
            }
            for key in ["anyOf", "oneOf"] {
                let Some(Value::Array(alternatives)) = fields.get(key) else {
                    continue;
                };
                let named: Vec<&Value> = alternatives.iter().filter(|one| !null(one)).collect();
                if named.len() == alternatives.len() {
                    continue;
                }
                if let [Value::Object(only)] = named[..] {
                    let only = only.clone();
                    fields.remove(key);
                    // What the argument says about itself wins over the type's own words.
                    for (name, value) in only {
                        fields.entry(name).or_insert(value);
                    }
                }
            }
            Value::Object(fields)
        }
        other => other,
    }
}

fn inline(value: Value, definitions: &Map<String, Value>) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| inline(item, definitions))
                .collect(),
        ),
        Value::Object(mut fields) => {
            let named = fields
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|path| path.strip_prefix("#/$defs/"))
                .and_then(|name| definitions.get(name));
            if let Some(Value::Object(definition)) = named {
                let definition = definition.clone();
                fields.remove("$ref");
                for (name, value) in definition {
                    fields.entry(name).or_insert(value);
                }
            }
            Value::Object(
                fields
                    .into_iter()
                    .map(|(name, value)| (name, inline(value, definitions)))
                    .collect(),
            )
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::plain;
    use serde_json::json;

    fn reduced(schema: &serde_json::Value) -> serde_json::Value {
        serde_json::Value::Object(plain(schema.as_object().unwrap()))
    }

    #[test]
    fn a_schema_keeps_what_a_model_needs_and_loses_what_only_a_validator_reads() {
        let schema = reduced(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false,
            "required": ["query"],
            "properties": {
                "query": {"type": "string", "description": "Words."},
                "limit": {"type": ["integer", "null"], "format": "uint", "minimum": 1, "maximum": 200, "default": null},
                "local": {"type": "boolean", "default": false},
                "chat": {"type": "array", "items": {"type": "string"}, "default": []},
                "detail": {"$ref": "#/$defs/Detail", "default": "compact", "description": "How much."},
                "unit": {"anyOf": [{"$ref": "#/$defs/Unit"}, {"type": "null"}], "description": "What to return."}
            },
            "$defs": {
                "Detail": {"type": "string", "enum": ["compact", "extended"]},
                "Unit": {"type": "string", "enum": ["chats", "messages"], "description": "A unit."}
            }
        }));
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": {"type": "string", "description": "Words."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200},
                    "local": {"type": "boolean"},
                    "chat": {"type": "array", "items": {"type": "string"}},
                    "detail": {"type": "string", "enum": ["compact", "extended"], "default": "compact", "description": "How much."},
                    "unit": {"type": "string", "enum": ["chats", "messages"], "description": "What to return."}
                }
            })
        );
    }

    #[test]
    fn alternatives_told_apart_by_a_constant_become_one_flat_object() {
        let expected = json!({"type": "object", "properties": {"id": {"type": "string"}}});
        let schema = reduced(&json!({
            "type": "object",
            "properties": {
                "change": {"description": "What to do.", "oneOf": [
                    {"type": "object", "required": ["action", "scope", "body"], "properties": {
                        "action": {"type": "string", "const": "create"}, "scope": {"type": "string"}, "body": {"type": "string"}}},
                    {"type": "object", "required": ["action", "expected", "body"], "properties": {
                        "action": {"type": "string", "const": "revise"}, "expected": expected, "body": {"type": "string"}}},
                    {"type": "object", "required": ["action", "expected"], "properties": {
                        "action": {"type": "string", "const": "forget"}, "expected": expected}}
                ]},
                "intent": {"oneOf": [
                    {"type": "string", "const": "by-words", "description": "Every word."},
                    {"type": "string", "const": "by-meaning", "description": "A question."}
                ]}
            }
        }));
        assert_eq!(
            schema["properties"]["change"],
            json!({
                "description": "What to do. create needs scope, body. revise needs expected, body. forget needs expected.",
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {"type": "string", "enum": ["create", "revise", "forget"]},
                    "scope": {"type": "string"},
                    "body": {"type": "string"},
                    "expected": expected
                }
            })
        );
        assert_eq!(
            schema["properties"]["intent"],
            json!({"type": "string", "enum": ["by-words", "by-meaning"],
                "description": "by-words: Every word. by-meaning: A question."})
        );
    }

    #[test]
    fn a_property_with_its_own_values_per_alternative_lists_them_all_and_says_whose() {
        let schema = reduced(
            &json!({"type": "object", "properties": {"details": {"oneOf": [
                {"type": "object", "required": ["kind", "state"], "properties": {
                    "kind": {"type": "string", "const": "decision"}, "state": {"type": "string", "enum": ["accepted", "superseded"]}}},
                {"type": "object", "required": ["kind", "state", "reported_by"], "properties": {
                    "kind": {"type": "string", "const": "fact"}, "reported_by": {"type": "string"},
                    "state": {"type": "string", "enum": ["current", "superseded"]}}}
            ]}}}),
        );
        assert_eq!(
            schema["properties"]["details"],
            json!({
                "type": "object",
                "required": ["kind"],
                "description": "decision needs state. fact needs state, reported_by. decision state: accepted, superseded. fact state: current, superseded.",
                "properties": {
                    "kind": {"type": "string", "enum": ["decision", "fact"]},
                    "state": {"type": "string", "enum": ["accepted", "superseded", "current"]},
                    "reported_by": {"type": "string"}
                }
            })
        );
    }

    #[test]
    fn alternatives_that_give_a_property_different_shapes_stay_apart() {
        let details = json!({"oneOf": [
            {"type": "object", "required": ["kind"], "properties": {
                "kind": {"type": "string", "const": "one"}, "value": {"type": "string"}}},
            {"type": "object", "required": ["kind"], "properties": {
                "kind": {"type": "string", "const": "many"}, "value": {"type": "array", "items": {"type": "string"}}}}
        ]});
        let schema = reduced(&json!({"type": "object", "properties": {"details": details}}));
        assert_eq!(schema["properties"]["details"], details);
    }

    #[test]
    fn a_choice_given_as_a_list_and_a_described_value_is_one_list() {
        let schema = reduced(
            &json!({"type": "object", "properties": {"purpose": {"default": "context", "oneOf": [
                {"type": "string", "enum": ["context", "report"]},
                {"type": "string", "const": "endorsement", "description": "The person's own approval."}
            ]}}}),
        );
        assert_eq!(
            schema["properties"]["purpose"],
            json!({"type": "string", "default": "context", "enum": ["context", "report", "endorsement"],
                "description": "endorsement: The person's own approval."})
        );
    }

    #[test]
    fn a_definition_used_only_inside_another_is_written_there() {
        let body = json!({"type": "object", "description": "x".repeat(120), "properties": {"details": {"$ref": "#/$defs/Details"}}});
        let schema = reduced(&json!({
            "type": "object",
            "properties": {"first": {"$ref": "#/$defs/Body"}, "second": {"$ref": "#/$defs/Body"}},
            "$defs": {"Body": body, "Details": {"type": "object", "description": "y".repeat(120)}}
        }));
        assert_eq!(
            schema["properties"]["first"]["properties"]["details"]["type"],
            "object"
        );
        assert_eq!(
            schema["properties"]["first"],
            schema["properties"]["second"]
        );
        assert!(schema.get("$defs").is_none());
    }

    #[test]
    fn definitions_that_name_each_other_are_left_as_they_were() {
        let schema = reduced(&json!({
            "type": "object",
            "properties": {"a": {"$ref": "#/$defs/A"}},
            "$defs": {
                "A": {"type": "object", "properties": {"b": {"$ref": "#/$defs/B"}}},
                "B": {"type": "object", "properties": {"a": {"$ref": "#/$defs/A"}}}
            }
        }));
        assert!(schema["$defs"].get("A").is_some() && schema["$defs"].get("B").is_some());
    }

    #[test]
    fn every_definition_is_written_in_place_except_one_that_names_itself() {
        let body = json!({"type": "object", "description": "x".repeat(120), "properties": {"statement": {"type": "string"}}});
        let schema = reduced(&json!({
            "type": "object",
            "properties": {
                "first": {"$ref": "#/$defs/Body"},
                "second": {"$ref": "#/$defs/Body"},
                "tree": {"$ref": "#/$defs/Tree"}
            },
            "$defs": {
                "Body": body,
                "Tree": {"type": "object", "properties": {"child": {"$ref": "#/$defs/Tree"}}},
                "Unused": {"type": "string"}
            }
        }));
        assert_eq!(schema["properties"]["first"], body);
        assert_eq!(schema["properties"]["second"], body);
        assert_eq!(
            schema["properties"]["tree"],
            json!({"$ref": "#/$defs/Tree"})
        );
        assert!(schema["$defs"].get("Tree").is_some());
        assert!(schema["$defs"].get("Body").is_none() && schema["$defs"].get("Unused").is_none());
    }
}
