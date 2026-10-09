use super::*;
use serde_json::json;

fn example() -> serde_json::Value {
    json!({"schema_version":2,"source_id":"work-chat","batch_id":"batch-1","expected_revision":0,
    "changes":[
        {"op":"upsert-object","id":"chat-1","role":"container","kind":"slack:channel",
         "title":"Storage decisions"},
        {"op":"upsert-object","id":"message-1","role":"entry","kind":"slack:message",
         "parent":"chat-1","author":{"id":"alice","name":"Alice","kind":"human"},
         "content":"Use Qdrant.","created_at":1_791_540_000_000_i64,
         "url":"https://chat.example/messages/1"}
    ]})
}

fn decode(value: &serde_json::Value) -> Result<Batch, Error> {
    Batch::decode(&serde_json::to_vec(value).unwrap())
}

fn push(value: &mut serde_json::Value, change: serde_json::Value) {
    value["changes"].as_array_mut().unwrap().push(change);
}

#[test]
fn complete_replacements_and_explicit_deletions_round_trip() {
    let mut value = example();
    push(&mut value, json!({"op":"delete-object","id":"old-message"}));
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
    assert_eq!(schema["$defs"]["ConnectorVersion"]["const"], 2);
    for field in ["content", "fingerprint", "provider"] {
        let mut value = example();
        value[field] = json!("not accepted");
        assert_eq!(decode(&value), Err(Error::InvalidJson));
    }
    for version in [0, 1, 3] {
        let mut value = example();
        value["schema_version"] = json!(version);
        assert_eq!(decode(&value), Err(Error::InvalidJson));
    }
    for op in ["execute-code", "upsert-chat", "upsert-message"] {
        let mut value = example();
        value["changes"][1]["op"] = json!(op);
        assert_eq!(decode(&value), Err(Error::InvalidJson));
    }
    let mut value = example();
    value["changes"][1]["role"] = json!("attachment");
    assert_eq!(decode(&value), Err(Error::InvalidJson));
}

#[test]
fn kinds_are_namespaced_lowercase_labels() {
    for kind in ["discord:reply-thread", "codex:turn", "a1:b2"] {
        let mut value = example();
        value["changes"][1]["kind"] = json!(kind);
        assert!(decode(&value).is_ok(), "{kind}");
    }
    for kind in [
        "message",
        "Discord:message",
        ":message",
        "slack:",
        "a:b:c",
        "slack:-x",
    ] {
        let mut value = example();
        value["changes"][1]["kind"] = json!(kind);
        assert_eq!(decode(&value), Err(Error::InvalidJson), "{kind}");
    }
}

#[test]
fn mutations_are_unambiguous_and_references_are_not_silently_rewritten() {
    for duplicate in [
        json!({"op":"delete-object","id":"message-1"}),
        example()["changes"][1].clone(),
    ] {
        let mut value = example();
        push(&mut value, duplicate);
        assert_eq!(decode(&value), Err(Error::DuplicateMutation));
    }
    // One identity namespace per source, whatever the role.
    let mut value = example();
    push(
        &mut value,
        json!({"op":"upsert-object","id":"message-1","role":"document",
        "kind":"notes:note","content":"Same ID"}),
    );
    assert_eq!(decode(&value), Err(Error::DuplicateMutation));
    let mut value = example();
    value["changes"][0] = json!({"op":"delete-object","id":"chat-1"});
    assert_eq!(decode(&value), Err(Error::ConflictingDeletion));
    let mut value = example();
    value["changes"][1]["reply_to"] = json!("message-1");
    assert_eq!(decode(&value), Err(Error::SelfReply));
    value["changes"][1]["reply_to"] = json!("older-message-outside-this-batch");
    assert!(decode(&value).is_ok()); // Stored targets are checked at ingestion.
}

#[test]
fn roles_decide_which_fields_an_object_may_carry() {
    let fields = [
        // A container or segment has no text, author, reply or attachments.
        (0, "content", json!("text")),
        (0, "author", json!({"id":"a","kind":"human"})),
        (0, "reply_to", json!("message-1")),
        (
            0,
            "attachments",
            json!([{"id":"f","name":"f","media_type":"text/plain",
            "url":"https://chat.example/f"}]),
        ),
        // An entry has text and a parent.
        (1, "content", serde_json::Value::Null),
        (1, "parent", serde_json::Value::Null),
    ];
    for (index, field, field_value) in fields {
        let mut value = example();
        value["changes"][index][field] = field_value;
        assert!(
            matches!(
                decode(&value),
                Err(Error::InvalidRoleFields | Error::InvalidParent)
            ),
            "{index} {field}"
        );
    }
    let mut value = example();
    value["changes"][0]["role"] = json!("segment");
    assert!(decode(&value).is_ok()); // Stored ancestry is checked at ingestion.
    let mut document = example();
    document["changes"][1] = json!({"op":"upsert-object","id":"note","role":"document",
        "kind":"notes:note","content":"A note","author":{"id":"a","kind":"human"}});
    assert_eq!(decode(&document), Err(Error::InvalidRoleFields));
    document["changes"][1]["author"] = serde_json::Value::Null;
    assert!(decode(&document).is_ok());
}

#[test]
fn parents_and_replies_form_acyclic_graphs_and_documents_contain_nothing() {
    let mut value = example();
    value["changes"][0]["parent"] = json!("message-1");
    assert_eq!(decode(&value), Err(Error::CyclicHierarchy));
    let mut value = example();
    push(
        &mut value,
        json!({"op":"upsert-object","id":"note","role":"document",
        "kind":"notes:note","content":"A note"}),
    );
    value["changes"][1]["parent"] = json!("note");
    assert_eq!(decode(&value), Err(Error::InvalidParent));
    let mut value = example();
    value["changes"][1]["reply_to"] = json!("message-2");
    push(
        &mut value,
        json!({"op":"upsert-object","id":"message-2","role":"entry",
        "kind":"slack:message","parent":"chat-1","content":"Second","reply_to":"message-1"}),
    );
    assert_eq!(decode(&value), Err(Error::CyclicHierarchy));
    value["changes"][2]["reply_to"] = json!("chat-1");
    assert_eq!(decode(&value), Err(Error::InvalidReply));
    value["changes"][2] = json!({"op":"delete-object", "id":"message-2"});
    assert_eq!(decode(&value), Err(Error::ConflictingDeletion));
}

#[test]
fn an_entry_may_own_a_thread_and_segments_structure_containers() {
    let fixture = include_str!("../../fixtures/external-connector-batch.json");
    let batch = Batch::decode(fixture.as_bytes()).unwrap();
    let Change::UpsertObject(thread) = &batch.changes[2] else {
        panic!("thread fixture")
    };
    assert_eq!(thread.role, Role::Container);
    assert_eq!(thread.kind.as_str(), "discord:reply-thread");
    assert_eq!(thread.parent.as_ref().unwrap().as_str(), "message-1");
    let mut value = example();
    push(
        &mut value,
        json!({"op":"upsert-object","id":"review-1","role":"segment",
        "kind":"github:review","parent":"chat-1","title":"Changes requested"}),
    );
    push(
        &mut value,
        json!({"op":"upsert-object","id":"comment-1","role":"entry",
        "kind":"github:review-comment","parent":"review-1","content":"Rename this."}),
    );
    assert!(decode(&value).is_ok());
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
        vec![json!({"op":"delete-object","id":"old"}); MAX_CHANGES + 1],
    ] {
        let mut value = example();
        value["changes"] = json!(changes);
        assert_eq!(decode(&value), Err(Error::InvalidChangeCount));
    }
}

#[test]
fn identities_and_links_do_not_accept_ambiguous_or_executable_inputs() {
    for id in [
        "",
        "message\nforged",
        " has-space",
        "ü",
        "-leading",
        "has#hash",
    ] {
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
    let Change::UpsertObject(message) = batch.changes.remove(1) else {
        panic!("message fixture")
    };
    let mut message = message;
    // Canonical JSON escapes these characters: raw content alone is below 8 MiB.
    message.content = Some("\0".repeat(100_000));
    for index in 0..20 {
        message.id = Id::try_from(format!("message-{index}")).unwrap();
        batch.changes.push(Change::UpsertObject(message.clone()));
    }
    assert_eq!(batch.validate(), Err(Error::BatchTooLarge));
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
fn multi_party_entries_preserve_authors_and_branching_replies() {
    let mut value = example();
    for (id, name, kind, parent) in [
        ("bob", "Bob", "human", "message-1"),
        ("carol", "Carol", "human", "message-1"),
        ("build-bot", "Build bot", "bot", "message-bob"),
    ] {
        push(
            &mut value,
            json!({"op":"upsert-object","id":format!("message-{id}"),
            "role":"entry","kind":"slack:message","parent":"chat-1",
            "author":{"id":id,"name":name,"kind":kind},
            "content":"A participant's reply","reply_to":parent}),
        );
    }
    let batch = decode(&value).unwrap();
    let entries = batch
        .changes
        .iter()
        .filter_map(|change| match change {
            Change::UpsertObject(object) if object.role == Role::Entry => Some(object),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[1].reply_to, entries[2].reply_to);
    assert_eq!(entries[3].author.as_ref().unwrap().kind, AuthorKind::Bot);
    assert_eq!(
        Batch::decode(&serde_json::to_vec(&batch).unwrap()).unwrap(),
        batch
    );
}

#[test]
fn metadata_remains_opaque_objects_with_explicit_budgets() {
    let mut value = example();
    value["changes"][0]["metadata"] = json!({"workspace":{"id":"work","name":"Company"},
        "channel":{"name":"engineering","private":true}});
    value["changes"][1]["metadata"] = json!({"reactions":[{"emoji":"👍","count":3}]});
    let batch = decode(&value).unwrap();
    let encoded = serde_json::to_value(batch).unwrap();
    for index in [0, 1] {
        assert_eq!(
            encoded["changes"][index]["metadata"],
            value["changes"][index]["metadata"]
        );
        let mut oversized = example();
        oversized["changes"][index]["metadata"] =
            json!({"opaque":"x".repeat(MAX_METADATA_BYTES+1)});
        assert_eq!(decode(&oversized), Err(Error::MetadataTooLarge));
        let mut invalid = example();
        invalid["changes"][index]["metadata"] = json!(["not an object"]);
        assert_eq!(decode(&invalid), Err(Error::InvalidJson));
    }
}

fn schema() -> serde_json::Value {
    json!({"op":"declare-schema","kinds":[
        {"kind":"slack:channel","role":"container","root":true},
        {"kind":"slack:thread","role":"container","parents":["slack:message"]},
        {"kind":"slack:message","role":"entry","parents":["slack:channel","slack:thread"]}
    ]})
}

#[test]
fn schemas_must_be_self_consistent() {
    let mut value = example();
    push(&mut value, schema());
    assert!(decode(&value).is_ok());
    for (index, field, field_value) in [
        // An undeclared parent kind.
        (2, "parents", json!(["slack:unknown"])),
        // An entry cannot be a root: it would sit outside every container.
        (2, "root", json!(true)),
        // A non-root kind needs somewhere to live.
        (1, "parents", json!([])),
    ] {
        let mut value = example();
        let mut declared = schema();
        declared["kinds"][index][field] = field_value;
        push(&mut value, declared);
        assert_eq!(decode(&value), Err(Error::InvalidSchema), "{index} {field}");
    }
    let mut duplicate = schema();
    duplicate["kinds"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"slack:channel","role":"segment","parents":["slack:channel"]}));
    let mut value = example();
    push(&mut value, duplicate);
    assert_eq!(decode(&value), Err(Error::InvalidSchema));
    let mut document_parent = schema();
    document_parent["kinds"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"slack:file","role":"document","root":true}));
    document_parent["kinds"][2]["parents"] = json!(["slack:file"]);
    let mut value = example();
    push(&mut value, document_parent);
    assert_eq!(decode(&value), Err(Error::InvalidSchema));
    let mut twice = example();
    push(&mut twice, schema());
    push(&mut twice, schema());
    assert_eq!(decode(&twice), Err(Error::DuplicateMutation));
}

#[test]
fn a_declared_schema_constrains_objects_in_the_same_batch() {
    let mut value = example();
    push(&mut value, schema());
    // A thread hangs off a message, as the schema allows.
    push(
        &mut value,
        json!({"op":"upsert-object","id":"thread-1","role":"container",
        "kind":"slack:thread","parent":"message-1"}),
    );
    assert!(decode(&value).is_ok());
    for (index, field, field_value) in [
        (1, "kind", json!("slack:reaction")), // undeclared kind
        (1, "parent", json!("thread-x-outside-batch")), // fine here; stored check later
        (0, "kind", json!("slack:thread")),   // a thread cannot be a root
    ] {
        let mut value = example();
        push(&mut value, schema());
        value["changes"][index][field] = field_value.clone();
        let decoded = decode(&value);
        if field_value == json!("thread-x-outside-batch") {
            assert!(decoded.is_ok());
        } else {
            assert_eq!(decoded, Err(Error::SchemaViolation), "{index} {field}");
        }
    }
}
