//! The dashboard's HTTP contract, as an `OpenAPI` document.
//!
//! Routes are registered with `aide`, which builds the router and the
//! document together: a route cannot be served without being described.
//! Query parameters and request bodies are read from each handler's own
//! extractors, and each description names the Rust type the route answers
//! with; schemas are derived from those types. `dashboard/openapi.json` is
//! the committed copy the front end's types are generated from, and a test
//! fails when it is out of date.
use aide::transform::TransformOperation;
use schemars::JsonSchema;
use serde_json::Value;

/// What a failed request answers with, whatever the route.
#[derive(JsonSchema)]
#[allow(dead_code, reason = "describes the error body; never constructed")]
pub(super) struct Failure {
    error: crate::ErrorBody,
}

/// `/api/services/fleet/jobs` read by GET is `getApiServicesFleetJobs`.
fn operation_id(method: &str, path: &str) -> String {
    let mut id = method.to_owned();
    for word in path.split(['/', '-', '_']).filter(|word| !word.is_empty()) {
        let (first, rest) = word.split_at(1);
        id.push_str(&first.to_uppercase());
        id.push_str(rest);
    }
    id
}

fn describe<'t, R: JsonSchema>(
    operation: TransformOperation<'t>,
    method: &str,
    path: &str,
    summary: &str,
) -> TransformOperation<'t> {
    operation
        .id(&operation_id(method, path))
        .summary(summary)
        .response::<200, axum::Json<R>>()
        .default_response::<axum::Json<Failure>>()
}

/// A GET of `path` that answers with `R`.
pub(super) fn reads<R: JsonSchema>(
    path: &'static str,
    summary: &'static str,
) -> impl FnOnce(TransformOperation<'_>) -> TransformOperation<'_> {
    move |operation| describe::<R>(operation, "get", path, summary)
}
/// A POST to `path` that answers with `R`.
pub(super) fn changes<R: JsonSchema>(
    path: &'static str,
    summary: &'static str,
) -> impl FnOnce(TransformOperation<'_>) -> TransformOperation<'_> {
    move |operation| describe::<R>(operation, "post", path, summary)
}

/// The contract as JSON, with each schema under a name of its own.
pub(super) fn document(api: &aide::openapi::OpenApi) -> Value {
    let mut document = serde_json::to_value(api).unwrap_or_default();
    crate::openapi::settle(&mut document, "Scope", &[("after", "CoverageScope")]);
    // The event stream is described by the JSON each event carries, but it is
    // not answered as JSON.
    for stream in [
        "/paths/~1api~1events/get/responses/200/content",
        "/paths/~1api~1assistant/post/responses/200/content",
    ] {
        if let Some(Value::Object(content)) = document.pointer_mut(stream)
            && let Some(event) = content.remove("application/json")
        {
            content.insert("text/event-stream".into(), event);
        }
    }
    crate::openapi::sorted(document)
}

#[cfg(test)]
mod tests {
    /// `dashboard/openapi.json` is what the front end's types are generated
    /// from. Rewrite it with `MASH_WRITE_CONTRACT=1` after changing a route or
    /// a type it answers with, then run `pnpm contract` in `dashboard/`.
    #[test]
    fn committed_contract_matches_the_routes() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dashboard/openapi.json");
        let mut current =
            serde_json::to_string_pretty(&super::document(&super::super::api().1)).unwrap();
        current.push('\n');
        if std::env::var_os("MASH_WRITE_CONTRACT").is_some() {
            std::fs::write(&path, &current).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == current,
            "dashboard/openapi.json is out of date: rerun this test with MASH_WRITE_CONTRACT=1, then `pnpm contract` in dashboard/"
        );
    }
}
