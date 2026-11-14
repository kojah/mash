//! The input schemas as tools publish them.
//!
//! A model reads every tool's schema in every session, so each one is reduced
//! to what tells a model how to call the tool. Generated schemas also say
//! things only a validator uses: that no other properties are allowed, the
//! machine width of a number, that an omitted value defaults to nothing, and
//! "this or null" for every optional argument. Arguments are still checked
//! against the full typed models when a tool is called.
use serde_json::{Map, Value};

/// A definition this short is clearer written where it is used.
const SMALL: usize = 90;

/// `schema` with the validator-only parts removed and small or single-use
/// definitions written in place.
pub(super) fn plain(schema: &Map<String, Value>) -> Map<String, Value> {
    let Value::Object(mut schema) = strip(Value::Object(schema.clone())) else {
        return schema.clone();
    };
    let stripped = schema.clone();
    // Inlining one definition can leave another used once.
    for _ in 0..8 {
        let definitions = match schema.get("$defs") {
            Some(Value::Object(definitions)) => definitions.clone(),
            _ => break,
        };
        let text = Value::Object(schema.clone()).to_string();
        let inlined: Map<String, Value> = definitions
            .iter()
            .filter(|(name, definition)| {
                let own = definition.to_string();
                let uses = text.matches(&reference(name)).count();
                // Only one that refers to no other: those it names are written
                // into it first, a round earlier, and a ring is never reached.
                !own.contains("\"#/$defs/") && (uses == 1 || own.len() < SMALL)
            })
            .map(|(name, definition)| (name.clone(), definition.clone()))
            .collect();
        if inlined.is_empty() {
            break;
        }
        let Value::Object(mut next) = inline(Value::Object(schema), &inlined) else {
            unreachable!("inlining keeps an object an object")
        };
        if let Some(Value::Object(definitions)) = next.get_mut("$defs") {
            definitions.retain(|name, _| !inlined.contains_key(name));
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
    // Definitions that name each other in a ring cannot be written in place.
    if dangling(&schema) {
        return stripped;
    }
    schema
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
    fn a_definition_used_only_inside_another_is_written_there() {
        let body = json!({"type": "object", "description": "x".repeat(120), "properties": {"details": {"$ref": "#/$defs/Details"}}});
        let schema = reduced(&json!({
            "type": "object",
            "properties": {"first": {"$ref": "#/$defs/Body"}, "second": {"$ref": "#/$defs/Body"}},
            "$defs": {"Body": body, "Details": {"type": "object", "description": "y".repeat(120)}}
        }));
        assert_eq!(
            schema["$defs"]["Body"]["properties"]["details"]["type"],
            "object"
        );
        assert!(schema["$defs"].get("Details").is_none());
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
    fn a_large_definition_used_twice_stays_shared_and_one_that_names_itself_is_left_alone() {
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
        assert_eq!(
            schema["properties"]["first"],
            json!({"$ref": "#/$defs/Body"})
        );
        assert_eq!(
            schema["properties"]["tree"],
            json!({"$ref": "#/$defs/Tree"})
        );
        assert!(schema["$defs"].get("Body").is_some() && schema["$defs"].get("Tree").is_some());
        assert!(schema["$defs"].get("Unused").is_none());
    }
}
