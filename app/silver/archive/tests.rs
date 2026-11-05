use super::*;

#[test]
fn version_one_archive_context_keeps_its_frozen_wire_contract() {
    // The whole record is deliberately a version-1 fixture, independent of
    // the runtime MessageContext type and the current SQLite representation.
    const RECORD: &str = r#"{"record":"message","attachments":[],"id":"child","conversation_id":"chat","ordinal":1,"role":"user","content":"sample","dialogue_text":null,"context":{"kind":"parent","ordinal":0},"created_at":null,"fingerprint":""}"#;
    let record: Record = serde_json::from_str(RECORD).unwrap();
    record.validate().unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), RECORD);
    for (wire, parent) in [
        (r#"{"kind":"linear"}"#, None),
        (r#"{"kind":"root"}"#, Some(-1)),
        (r#"{"kind":"parent","ordinal":0}"#, Some(0)),
    ] {
        let context: Context = serde_json::from_str(wire).unwrap();
        assert_eq!(context.parent(), parent);
        assert_eq!(Context::from_parent(parent).unwrap(), context);
        assert_eq!(serde_json::to_string(&context).unwrap(), wire);
    }
    let invalid: Record = serde_json::from_str(&RECORD.replace(
        r#""context":{"kind":"parent","ordinal":0}"#,
        r#""context":{"kind":"parent","ordinal":1}"#,
    ))
    .unwrap();
    assert!(invalid.validate().is_err());
    assert!(Context::from_parent(Some(-2)).is_err());
}
