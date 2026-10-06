use super::*;

fn thread() -> (ConversationRecord, Vec<MessageRecord>) {
    let conversation = serde_json::from_value(serde_json::json!(
        {"id":"thread","provider":"codex","title":"Topic","project":null,"repository":null,"created_at":1,"updated_at":2}
    ))
    .unwrap();
    let messages = serde_json::from_value(serde_json::json!([
        {"id":"z-visible","conversation_id":"thread","ordinal":0,"role":"user","content":"raw source","projection":"visible dialogue","created_at":1,"generation":"model","vector":[127.0,0.0]},
        {"id":"a-hidden","conversation_id":"thread","ordinal":1,"role":"tool","content":"hidden evidence","projection":"","created_at":2,"generation":null,"vector":null}
    ]))
    .unwrap();
    (conversation, messages)
}

fn digest(
    conversation: &ConversationRecord,
    messages: &[MessageRecord],
) -> Result<String, AppError> {
    let mut fingerprint = Fingerprint::new(conversation)?;
    for message in messages {
        fingerprint.push(message)?;
    }
    Ok(fingerprint.finish())
}

#[test]
fn dependency_digest_ignores_gold_but_covers_all_silver_and_has_stable_order() {
    let (conversation, mut messages) = thread();
    let before = digest(&conversation, &messages).unwrap();
    messages[0].vector = Some(vec![0.0, -127.0]);
    messages[0].generation = Some("another model".into());
    assert_eq!(digest(&conversation, &messages).unwrap(), before);
    let reversed = [messages[1].clone(), messages[0].clone()];
    assert!(
        digest(&conversation, &reversed).is_err(),
        "ordinals must ascend"
    );
    for defect in 0..7 {
        let (mut conversation, mut messages) = thread();
        match defect {
            0 => conversation.title = Some("other title".into()),
            1 => messages[0].content.push('!'),
            2 => messages[0].projection.push('!'),
            3 => messages[1].content.push('!'),
            4 => {
                messages[1].context = crate::message_context::MessageContext::Parent { ordinal: 0 };
            }
            5 => conversation.repository = Some("repo".into()),
            _ => {
                messages.pop();
            }
        }
        assert_ne!(
            digest(&conversation, &messages).unwrap(),
            before,
            "defect {defect}"
        );
    }
}

#[test]
fn publication_plans_validate_thread_fingerprints() {
    let mut target = Target::new(
        Build::default(),
        "model".into(),
        2,
        "version".into(),
        Some(("instance".into(), 3)),
        BTreeMap::from([("thread".into(), "a".repeat(64))]),
    );
    target.validate().unwrap();
    assert_eq!(target.publication_revision, 3);
    target.threads.insert("other".into(), "not-a-digest".into());
    assert!(target.validate().is_err());
}
