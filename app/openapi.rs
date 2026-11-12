//! Naming for generated `OpenAPI` documents.
//!
//! Several types share a short name (`Request`, `Scope`), and the MCP tools
//! publish them under those names, so the types cannot be renamed. One
//! document cannot hold two schemas of one name: the generator numbers the
//! later ones (`Request2`). [`settle`] gives each a name of its own instead.
use serde_json::Value;

const REFERENCE: &str = "#/components/schemas/";

/// Points every reference to the schema `from` at `to`.
fn redirect(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::Object(fields) => {
            if let Some(Value::String(target)) = fields.get_mut("$ref")
                && target.strip_prefix(REFERENCE) == Some(from)
            {
                *target = format!("{REFERENCE}{to}");
            }
            fields
                .values_mut()
                .for_each(|field| redirect(field, from, to));
        }
        Value::Array(items) => items.iter_mut().for_each(|item| redirect(item, from, to)),
        _ => {}
    }
}

fn rename(document: &mut Value, from: &str, to: &str) {
    if from != to
        && let Some(schemas) = document["components"]["schemas"].as_object_mut()
        && let Some(schema) = schemas.remove(from)
    {
        schemas.insert(to.into(), schema);
        redirect(document, from, to);
    }
}

/// Among the schemas the generator called `base`, `base2`, `base3`…, names
/// each one that has the property `marker` after its `name`. When that leaves
/// a single schema, it takes the plain `base` name.
pub(crate) fn settle(document: &mut Value, base: &str, named: &[(&str, &str)]) {
    let candidates = |document: &Value| -> Vec<String> {
        document["components"]["schemas"]
            .as_object()
            .into_iter()
            .flat_map(|schemas| schemas.keys())
            .filter(|name| {
                name.strip_prefix(base)
                    .is_some_and(|rest| rest.chars().all(|digit| digit.is_ascii_digit()))
            })
            .cloned()
            .collect()
    };
    for candidate in candidates(document) {
        let properties = &document["components"]["schemas"][&candidate]["properties"];
        if let Some((_, name)) = named
            .iter()
            .find(|(marker, _)| properties.get(marker).is_some())
        {
            rename(document, &candidate, name);
        }
    }
    if let [only] = candidates(document).as_slice() {
        rename(document, only, base);
    }
}

/// Orders the schemas by name, so the document does not depend on the order
/// its routes were described in.
pub(crate) fn sorted(mut document: Value) -> Value {
    if let Some(schemas) = document["components"]["schemas"].as_object_mut() {
        schemas.sort_keys();
    }
    document
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn numbered_schemas_get_their_own_names_and_references_follow() {
        let mut document = json!({
            "paths": { "/a": { "schema": { "$ref": "#/components/schemas/Request2" } } },
            "components": { "schemas": {
                "Request": { "properties": { "direction": {} } },
                "Request2": { "properties": { "other": { "$ref": "#/components/schemas/Request" } } },
                "Requested": {},
            } },
        });
        super::settle(&mut document, "Request", &[("direction", "ListRequest")]);
        assert_eq!(
            document,
            json!({
                "paths": { "/a": { "schema": { "$ref": "#/components/schemas/Request" } } },
                "components": { "schemas": {
                    "ListRequest": { "properties": { "direction": {} } },
                    "Request": { "properties": { "other": { "$ref": "#/components/schemas/ListRequest" } } },
                    "Requested": {},
                } },
            })
        );
    }
}
