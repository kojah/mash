use super::*;

fn thread() -> (ChatRecord, Vec<MessageRecord>) {
    let chat = serde_json::from_value(serde_json::json!(
        {"id":"thread","provider":"codex","title":"Topic","project":null,"repository":null,"created_at":1,"updated_at":2}
    ))
    .unwrap();
    let messages = serde_json::from_value(serde_json::json!([
        {"id":"z-visible","chat_id":"thread","ordinal":0,"role":"user","content":"raw source","projection":"visible dialogue","created_at":1,"generation":"model","vector":[127.0,0.0]},
        {"id":"a-hidden","chat_id":"thread","ordinal":1,"role":"tool","content":"hidden evidence","projection":"","created_at":2,"generation":null,"vector":null}
    ]))
    .unwrap();
    (chat, messages)
}

fn digest(chat: &ChatRecord, messages: &[MessageRecord]) -> Result<String, AppError> {
    let mut fingerprint = Fingerprint::new(chat)?;
    for message in messages {
        fingerprint.push(message)?;
    }
    Ok(fingerprint.finish())
}

#[test]
fn dependency_digest_ignores_views_but_covers_all_silver_and_has_stable_order() {
    let (chat, mut messages) = thread();
    let before = digest(&chat, &messages).unwrap();
    messages[0].vector = Some(vec![0.0, -127.0]);
    messages[0].generation = Some("another model".into());
    assert_eq!(digest(&chat, &messages).unwrap(), before);
    let reversed = [messages[1].clone(), messages[0].clone()];
    assert!(digest(&chat, &reversed).is_err(), "ordinals must ascend");
    for defect in 0..7 {
        let (mut chat, mut messages) = thread();
        match defect {
            0 => chat.title = Some("other title".into()),
            1 => messages[0].content.push('!'),
            2 => messages[0].projection.push('!'),
            3 => messages[1].content.push('!'),
            4 => {
                messages[1].context = crate::message_context::MessageContext::Parent {
                    id: messages[0].id.clone(),
                };
            }
            5 => chat.repository = Some("repo".into()),
            _ => {
                messages.pop();
            }
        }
        assert_ne!(digest(&chat, &messages).unwrap(), before, "defect {defect}");
    }
}
