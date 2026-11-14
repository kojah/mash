use super::*;
use serde_json::json;

#[test]
fn envelope_round_trips_opaque_inputs_and_absent_error_flags() {
    let encoded = r#"{"tool_calls":[{"name":"exec","call_id":"c1","input":{"cmd":["ls",null,3]}},{"name":"apply_patch","call_id":"c2","input":"*** Begin Patch"}],"tool_results":[{"call_id":"c1","is_error":true},{"call_id":"c2"}]}"#;
    let activity = Activity::decode(Some(encoded)).unwrap();
    assert_eq!(activity.tool_calls.len(), 2);
    assert_eq!(
        activity.tool_calls[0].input,
        Some(json!({"cmd": ["ls", null, 3]}))
    );
    assert_eq!(activity.tool_calls[1].input, Some(json!("*** Begin Patch")));
    assert_eq!(activity.tool_results[0].is_error, Some(true));
    assert_eq!(activity.tool_results[1].is_error, None);
    assert_eq!(activity.encode().unwrap().as_deref(), Some(encoded));
    activity.validate().unwrap();
    assert_eq!(Activity::default().encode().unwrap(), None);
    assert!(Activity::decode(None).unwrap().is_empty());
    assert!(Activity::decode(Some(r#"{"tool_calls":[],"future":1}"#)).is_err());
}

#[test]
fn validation_bounds_labels_and_redaction_keeps_inputs_structured() {
    assert!(
        Activity::call(Some("bad\nname"), None, None)
            .validate()
            .is_err()
    );
    assert!(Activity::call(Some(""), None, None).validate().is_err());
    let token = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
    let mut activity = Activity::call(
        Some("exec"),
        Some("c1"),
        Some(&json!({"env": {"TOKEN": token}, "args": [token, 1]})),
    );
    activity.redact();
    let input = activity.tool_calls[0].input.as_ref().unwrap();
    assert!(!input.to_string().contains(token), "{input}");
    assert!(input["env"]["TOKEN"].is_string());
    assert_eq!(input["args"][1], json!(1));
}
