use super::*;

fn fixture() -> (Loaded, Identity, Build) {
    let identity = Identity {
        group_id: "group".into(),
        node_id: "node".into(),
        generation: "model".into(),
        dimensions: 2,
    };
    let build = Build {
        max_embedding_chars: None,
        representations: crate::gold::config::Representations::LexicalAndDense,
        text_preparation: crate::gold::config::TextPreparation::Preserve,
        embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
    };
    let silver = crate::shared::silver_revision::State::next(None, "a".repeat(64)).unwrap();
    let manifest =
        crate::shared::gold_manifest::Manifest::new(build.clone(), "model".into(), silver.clone());
    let head: Head = serde_json::from_value(serde_json::json!({
        "gold_manifest":manifest,"silver_revision":silver,"instance":"instance",
        "generation":"model","revision":1,"version":"head",
        "counts":{"conversations":1,"messages":2,"searchable_messages":1,"embeddings":1},
        "last_synced_at":null,"last_updated_at":null
    }))
    .unwrap();
    let data: Data = serde_json::from_value(serde_json::json!({
        "conversations":{"thread":{"id":"thread","provider":"codex","title":"Topic","project":null,"repository":null,"created_at":1,"updated_at":2}},
        "messages":{
            "z-visible":{"id":"z-visible","conversation_id":"thread","ordinal":0,"role":"user","content":"raw source","projection":"visible dialogue","created_at":1,"generation":"model","vector":[127.0,0.0]},
            "a-hidden":{"id":"a-hidden","conversation_id":"thread","ordinal":1,"role":"tool","content":"hidden evidence","projection":"","created_at":2,"generation":null,"vector":null}
        },"sources":{},"tombstones":{}
    })).unwrap();
    (
        Loaded {
            head: Some(head),
            expected: "head".into(),
            data,
        },
        identity,
        build,
    )
}

#[test]
fn dependency_digest_ignores_gold_but_covers_all_silver_and_has_stable_order() {
    let (mut loaded, _, _) = fixture();
    let before = fingerprints(&loaded.data).unwrap();
    let visible = loaded.data.messages.get_mut("z-visible").unwrap();
    visible.vector = Some(vec![0.0, -127.0]);
    visible.generation = Some("another model".into());
    assert_eq!(fingerprints(&loaded.data).unwrap(), before);
    let mut streaming = Fingerprint::new(&loaded.data.conversations["thread"]).unwrap();
    streaming.push(&loaded.data.messages["z-visible"]).unwrap();
    streaming.push(&loaded.data.messages["a-hidden"]).unwrap();
    assert_eq!(streaming.finish(), before["thread"]);
    for defect in 0..7 {
        let (mut changed, _, _) = fixture();
        match defect {
            0 => {
                changed.data.conversations.get_mut("thread").unwrap().title =
                    Some("other title".into());
            }
            1 => changed
                .data
                .messages
                .get_mut("z-visible")
                .unwrap()
                .content
                .push('!'),
            2 => changed
                .data
                .messages
                .get_mut("z-visible")
                .unwrap()
                .projection
                .push('!'),
            3 => changed
                .data
                .messages
                .get_mut("a-hidden")
                .unwrap()
                .content
                .push('!'),
            4 => {
                changed.data.messages.get_mut("a-hidden").unwrap().context =
                    crate::message_context::MessageContext::Parent { ordinal: 0 };
            }
            5 => {
                changed
                    .data
                    .conversations
                    .get_mut("thread")
                    .unwrap()
                    .repository = Some("repo".into());
            }
            _ => {
                changed.data.messages.remove("a-hidden");
            }
        }
        assert_ne!(
            fingerprints(&changed.data).unwrap(),
            before,
            "defect {defect}"
        );
    }
}

#[test]
fn reuse_requires_current_evidence_and_restores_only_unchanged_complete_threads() {
    let (mut loaded, identity, build) = fixture();
    let target = Target::new(&loaded, &identity, build.clone()).unwrap();
    target.validate().unwrap();
    let mut retained = Retained::take(&mut loaded, &identity, &build).unwrap();
    assert!(loaded.data.messages["z-visible"].vector.is_none());
    for message in loaded.data.messages.values_mut() {
        message.generation = None;
    }
    retained
        .request("thread".into(), &target.threads["thread"])
        .unwrap();
    assert!(
        retained
            .request("thread".into(), &target.threads["thread"])
            .is_err()
    );
    retained.restore(&mut loaded.data, &identity).unwrap();
    assert_eq!(
        loaded.data.messages["z-visible"].vector,
        Some(vec![127.0, 0.0])
    );
    assert_eq!(
        loaded.data.messages["z-visible"].generation.as_deref(),
        Some("model")
    );
    assert!(loaded.data.messages["a-hidden"].vector.is_none());
    assert!(loaded.data.messages["a-hidden"].generation.is_none());

    for defect in 0..4 {
        let (mut changed, identity, build) = fixture();
        match defect {
            0 => {
                changed
                    .head
                    .as_mut()
                    .unwrap()
                    .gold_manifest
                    .as_mut()
                    .unwrap()
                    .schema_version += 1;
            }
            1 => {
                changed
                    .head
                    .as_mut()
                    .unwrap()
                    .gold_manifest
                    .as_mut()
                    .unwrap()
                    .build = Build::default();
            }
            2 => changed.data.messages.get_mut("z-visible").unwrap().vector = None,
            _ => {
                changed
                    .data
                    .messages
                    .get_mut("z-visible")
                    .unwrap()
                    .generation = Some("other-model".into());
            }
        }
        assert_eq!(
            Target::new(&changed, &identity, build)
                .unwrap()
                .threads
                .len(),
            0,
            "defect {defect}"
        );
    }
}

#[test]
fn forged_changed_and_ambiguous_reuse_fail_closed() {
    for defect in 0..5 {
        let (mut loaded, identity, build) = fixture();
        let target = Target::new(&loaded, &identity, build.clone()).unwrap();
        let mut retained = Retained::take(&mut loaded, &identity, &build).unwrap();
        for message in loaded.data.messages.values_mut() {
            message.generation = None;
        }
        if defect == 0 {
            assert!(
                retained
                    .request("unknown".into(), &target.threads["thread"])
                    .is_err()
            );
            continue;
        }
        retained
            .request("thread".into(), &target.threads["thread"])
            .unwrap();
        match defect {
            1 => loaded
                .data
                .messages
                .get_mut("a-hidden")
                .unwrap()
                .content
                .push('!'),
            2 => {
                loaded.data.conversations.get_mut("thread").unwrap().title = Some("Changed".into());
            }
            3 => loaded.data.messages.get_mut("z-visible").unwrap().vector = Some(vec![127.0, 0.0]),
            _ => {
                loaded.data.conversations.clear();
                loaded.data.messages.clear();
            }
        }
        assert!(
            retained.restore(&mut loaded.data, &identity).is_err(),
            "defect {defect}"
        );
    }
}
