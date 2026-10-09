use super::*;
use serde_json::json;

fn example() -> serde_json::Value {
    json!({"schema_version":1,"source_id":"work-chat","batch_id":"batch-1","expected_revision":0,
    "changes":[
        {"op":"upsert-chat","id":"document-1","title":"Storage decisions"},
        {"op":"upsert-message","id":"message-1","chat_id":"document-1",
         "author":{"id":"alice","name":"Alice","kind":"human"},
             "content":"Use Qdrant.","created_at":1_791_540_000_000_i64,
         "url":"https://chat.example/messages/1"}
    ]})
}

fn decode(value: &serde_json::Value) -> Result<Batch, Error> {
    Batch::decode(&serde_json::to_vec(value).unwrap())
}

#[test]
fn complete_replacements_and_explicit_deletions_round_trip() {
    let mut value = example();
    value["changes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"op":"delete-message","id":"old-message"}));
    let batch = decode(&value).unwrap();
    assert_eq!(batch.source_id.as_str(), "work-chat");
    let encoded = serde_json::to_vec(&batch).unwrap();
    assert_eq!(Batch::decode(&encoded).unwrap(), batch);
    assert_eq!(batch.changes.len(), 3);
    let receipt = Receipt {
        schema_version: Version,
        source_id: batch.source_id,
        batch_id: batch.batch_id,
        source_revision: 1,
        silver_revision: 1,
        gold_ready: false,
    };
    let encoded = serde_json::to_vec(&receipt).unwrap();
    assert_eq!(
        serde_json::from_slice::<Receipt>(&encoded).unwrap(),
        receipt
    );
}

#[test]
fn schema_exposes_closed_fields_and_decoder_rejects_contract_drift() {
    let schema = serde_json::to_value(schemars::schema_for!(Batch)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["changes"]["maxItems"], MAX_CHANGES);
    assert_eq!(schema["$defs"]["ConnectorVersion"]["const"], 1);
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("expected_revision"))
    );
    for field in ["content", "fingerprint", "provider"] {
        let mut value = example();
        value[field] = json!("not accepted");
        assert_eq!(decode(&value), Err(Error::InvalidJson));
    }
    for version in [0, 2] {
        let mut value = example();
        value["schema_version"] = json!(version);
        assert_eq!(decode(&value), Err(Error::InvalidJson));
    }
    let mut value = example();
    value["changes"][1]["op"] = json!("execute-code");
    assert_eq!(decode(&value), Err(Error::InvalidJson));
}

#[test]
fn mutations_are_unambiguous_and_references_are_not_silently_rewritten() {
    for duplicate in [
        json!({"op":"delete-message","id":"message-1"}),
        example()["changes"][1].clone(),
    ] {
        let mut value = example();
        value["changes"].as_array_mut().unwrap().push(duplicate);
        assert_eq!(decode(&value), Err(Error::DuplicateMutation));
    }
    let mut value = example();
    value["changes"][0] = json!({"op":"delete-chat","id":"document-1"});
    assert_eq!(decode(&value), Err(Error::ConflictingChatDeletion));
    let mut value = example();
    value["changes"][1]["reply_to"] = json!("message-1");
    assert_eq!(decode(&value), Err(Error::SelfReply));
    value["changes"][1]["reply_to"] = json!("older-message-outside-this-batch");
    assert!(decode(&value).is_ok()); // Stored-parent validation belongs to ingestion.
}

#[test]
fn wire_and_record_budgets_are_enforced_before_storage() {
    assert_eq!(
        Batch::decode(&vec![b' '; MAX_BATCH_BYTES + 1]),
        Err(Error::BatchTooLarge)
    );
    let mut value = example();
    value["changes"][1]["content"] = json!("x".repeat(MAX_CONTENT_BYTES + 1));
    assert_eq!(decode(&value), Err(Error::FieldTooLarge));
    for changes in [
        vec![],
        vec![json!({"op":"delete-message","id":"old"}); MAX_CHANGES + 1],
    ] {
        let mut value = example();
        value["changes"] = json!(changes);
        assert_eq!(decode(&value), Err(Error::InvalidChangeCount));
    }
}

#[test]
fn identities_and_links_do_not_accept_ambiguous_or_executable_inputs() {
    for id in ["", "message\nforged", " has-space", "ü", "-leading"] {
        let mut value = example();
        value["changes"][1]["id"] = json!(id);
        assert_eq!(decode(&value), Err(Error::InvalidJson));
    }
    for link in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://user:secret@example.com/message",
    ] {
        let mut value = example();
        value["changes"][1]["url"] = json!(link);
        assert_eq!(decode(&value), Err(Error::InvalidUrl));
    }
}

#[test]
fn decoding_failures_never_echo_private_message_text() {
    let mut value = example();
    value["changes"][1]["op"] = json!("PRIVATE-CANARY");
    let error = decode(&value).unwrap_err();
    assert!(!error.to_string().contains("PRIVATE-CANARY"));
}

#[test]
fn direct_batches_obey_encoded_size_budget_without_a_second_payload_buffer() {
    let mut batch = decode(&example()).unwrap();
    let Change::UpsertMessage(message) = &batch.changes[1] else {
        panic!("message fixture")
    };
    let mut message = message.clone();
    // Canonical JSON escapes these characters: raw content alone is below 8 MiB.
    message.content = "\0".repeat(100_000);
    for index in 0..20 {
        message.id = Id::try_from(format!("message-{index}")).unwrap();
        batch.changes.push(Change::UpsertMessage(message.clone()));
    }
    // Remove the original message to keep all native identities unique.
    batch.changes.remove(1);
    assert_eq!(batch.validate(), Err(Error::BatchTooLarge));
}

#[test]
fn external_connector_batch_fixture_is_accepted_by_the_real_rust_decoder() {
    let example = include_str!("../../fixtures/external-connector-batch.json");
    assert!(Batch::decode(example.as_bytes()).is_ok());
}

#[test]
fn attachment_references_round_trip_without_fetching_and_reject_ambiguous_ids() {
    let mut value = example();
    let attachment = json!({"id":"file-1","name":"diagram.png","media_type":"image/png",
        "url":"https://chat.example/files/1","bytes":1234});
    value["changes"][1]["attachments"] = json!([attachment]);
    assert!(decode(&value).is_ok());
    value["changes"][1]["attachments"] = json!([attachment, attachment]);
    assert_eq!(decode(&value), Err(Error::InvalidAttachment));
    value["changes"][1]["attachments"] = json!([attachment]);
    value["changes"][1]["attachments"][0]["media_type"] = json!("image/png\nforged");
    assert_eq!(decode(&value), Err(Error::InvalidAttachment));
}

#[test]
fn multi_party_chats_preserve_authors_and_branching_replies() {
    let mut value = example();
    let changes = value["changes"].as_array_mut().unwrap();
    for (id, name, kind, parent) in [
        ("bob", "Bob", "human", "message-1"),
        ("carol", "Carol", "human", "message-1"),
        ("build-bot", "Build bot", "bot", "message-bob"),
    ] {
        changes.push(json!({"op":"upsert-message","id":format!("message-{id}"),
            "chat_id":"document-1","author":{"id":id,"name":name,"kind":kind},
            "content":"A participant's reply","reply_to":parent}));
    }
    let batch = decode(&value).unwrap();
    let messages = batch
        .changes
        .iter()
        .filter_map(|change| match change {
            Change::UpsertMessage(message) => Some(message),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].author.as_ref().unwrap().id.as_str(), "alice");
    assert_eq!(messages[1].author.as_ref().unwrap().id.as_str(), "bob");
    assert_eq!(messages[2].author.as_ref().unwrap().id.as_str(), "carol");
    assert_eq!(messages[1].reply_to, messages[2].reply_to);
    assert_eq!(messages[3].author.as_ref().unwrap().kind, AuthorKind::Bot);
    assert_eq!(
        messages[3].reply_to.as_ref().unwrap().as_str(),
        "message-bob"
    );
    assert_eq!(
        Batch::decode(&serde_json::to_vec(&batch).unwrap()).unwrap(),
        batch
    );
}

#[test]
fn chats_only_group_chat_messages_and_do_not_own_authorship_or_content() {
    for field in ["author", "content", "body"] {
        let mut value = example();
        value["changes"][0][field] = json!("not a document field");
        assert_eq!(decode(&value), Err(Error::InvalidJson));
    }
    let mut value = example();
    value["changes"][1]["author"] = serde_json::Value::Null;
    value["changes"][1]["content"] = json!("A file's text is still a message.");
    assert!(decode(&value).is_ok());
    value["changes"][1]["chat_id"] = serde_json::Value::Null;
    assert_eq!(decode(&value), Err(Error::InvalidJson));
}

#[test]
fn reply_graph_rejects_cycles_deleted_parents_and_cross_context_links() {
    let mut value = example();
    value["changes"][1]["reply_to"] = json!("message-2");
    value["changes"].as_array_mut().unwrap().push(json!({
        "op":"upsert-message", "id":"message-2", "chat_id":"document-1",
        "content":"Second reply", "reply_to":"message-1"
    }));
    assert_eq!(decode(&value), Err(Error::CyclicHierarchy));
    value["changes"][2]["reply_to"] = serde_json::Value::Null;
    assert!(decode(&value).is_ok());
    value["changes"][2]["chat_id"] = json!("another-context");
    assert_eq!(decode(&value), Err(Error::CrossChatReply));
    value["changes"][2] = json!({"op":"delete-message", "id":"message-2"});
    assert_eq!(decode(&value), Err(Error::ConflictingMessageDeletion));
}

#[test]
fn chat_and_message_metadata_remain_opaque_objects_with_explicit_budgets() {
    let mut value = example();
    value["changes"][0]["metadata"] = json!({"native_kind":"pull-request","number":42,
        "custom":{"labels":["bug","urgent"],"merged":false}});
    value["changes"][1]["metadata"] = json!({"reactions":[{"emoji":"👍","count":3}],
        "blocks":[{"type":"code","text":"anything source-specific"}]});
    let batch = decode(&value).unwrap();
    let encoded = serde_json::to_value(batch).unwrap();
    for index in [0, 1] {
        assert_eq!(
            encoded["changes"][index]["metadata"],
            value["changes"][index]["metadata"]
        );
    }
    for index in [0, 1] {
        let mut oversized = example();
        oversized["changes"][index]["metadata"] =
            json!({"opaque":"x".repeat(MAX_METADATA_BYTES+1)});
        assert_eq!(decode(&oversized), Err(Error::MetadataTooLarge));
        let mut invalid = example();
        invalid["changes"][index]["metadata"] = json!(["not an object"]);
        assert_eq!(decode(&invalid), Err(Error::InvalidJson));
    }
}

#[test]
fn a_channel_is_a_chat_with_workspace_details_in_metadata() {
    let mut value = example();
    value["changes"][0]["id"] = json!("channel-42");
    value["changes"][0]["title"] = json!("engineering");
    value["changes"][0]["metadata"] = json!({"workspace":{"id":"work","name":"Company"},
        "native_kind":"channel", "channel":{"name":"engineering","private":true}});
    value["changes"][1]["chat_id"] = json!("channel-42");
    let batch = decode(&value).unwrap();
    let Change::UpsertChat(chat) = &batch.changes[0] else {
        panic!("chat fixture")
    };
    assert_eq!(chat.id.as_str(), "channel-42");
    assert_eq!(chat.title.as_deref(), Some("engineering"));
    assert_eq!(
        serde_json::to_value(batch).unwrap()["changes"][0]["metadata"],
        value["changes"][0]["metadata"]
    );
}
