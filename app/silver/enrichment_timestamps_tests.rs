use super::*;

#[test]
fn selected_message_boundaries_are_traceable_and_legacy_evidence_is_not_invented() {
    let mut thread = Conversation {
        id: "thread".into(),
        provider: Provider::Codex,
        source: super::super::SourceIdentity::file("fixture"),
        title_origin: None,
        timestamp_origins: None,
        claude_lineage: None,
        title: None,
        project: None,
        repository: None,
        created_at: Some(10),
        updated_at: Some(20),
        messages: [("later", 20), ("earlier", 10)]
            .into_iter()
            .enumerate()
            .map(
                |(ordinal, (id, timestamp))| super::super::NormalizedMessage {
                    attachments: Vec::new(),
                    id: id.into(),
                    ordinal: i64::try_from(ordinal).unwrap(),
                    role: crate::role::Role::User,
                    context: crate::message_context::MessageContext::Linear,
                    content: "payload".into(),
                    dialogue_text: None,
                    created_at: Some(timestamp),
                },
            )
            .collect::<Vec<_>>()
            .into(),
    };
    for provider in [Provider::Codex, Provider::ClaudeCode] {
        thread.provider = provider;
        let captured = Evidence::capture(&thread).unwrap();
        let times = captured.timestamps.as_ref().unwrap().clone();
        assert_eq!(
            times.created_at,
            Some(TimestampOrigin::MessageBoundary {
                message_id: "earlier".into(),
                selection: Boundary::Minimum,
                rule_version: VERSION,
            })
        );
        assert_eq!(
            times.updated_at,
            Some(TimestampOrigin::MessageBoundary {
                message_id: "later".into(),
                selection: Boundary::Maximum,
                rule_version: VERSION,
            })
        );
        assert!(captured.is_current_for(provider));
        let mut legacy = captured.clone();
        legacy.timestamps = None;
        let encoded = serde_json::to_string(&legacy).unwrap();
        assert!(!encoded.contains("timestamps"));
        let decoded: Evidence = serde_json::from_str(&encoded).unwrap();
        assert!(decoded.is_current());
        assert!(!decoded.is_current_for(provider));
        let mut wrong = captured;
        wrong.timestamps.as_mut().unwrap().created_at = times.updated_at.clone();
        assert!(wrong.is_current());
        assert!(!wrong.is_current_for(provider));
    }
    thread.created_at = Some(99);
    assert_eq!(
        Evidence::capture(&thread)
            .unwrap()
            .timestamps
            .unwrap()
            .created_at,
        Some(TimestampOrigin::Unknown)
    );
}

#[test]
fn timestamp_rules_and_references_are_validated_without_reinterpreting_values() {
    for (provider, field) in [
        (Provider::Hermes, TimestampField::SessionTimestamp),
        (Provider::Pi, TimestampField::SessionsStartedAt),
        (Provider::GeminiCli, TimestampField::SessionTimestamp),
    ] {
        for origin in [
            TimestampOrigin::Unknown,
            TimestampOrigin::SourceMetadata {
                field,
                rule_version: VERSION,
            },
            TimestampOrigin::MessageBoundary {
                message_id: "message".into(),
                selection: Boundary::Minimum,
                rule_version: VERSION,
            },
        ] {
            let times = Timestamps {
                created_at: None,
                updated_at: Some(origin),
            };
            assert!(times.is_current());
            assert!(!times.is_current_for(provider));
        }
    }
    for origin in [
        TimestampOrigin::SourceMetadata {
            field: TimestampField::SessionTimeCreated,
            rule_version: VERSION + 1,
        },
        TimestampOrigin::MessageBoundary {
            message_id: String::new(),
            selection: Boundary::Minimum,
            rule_version: VERSION,
        },
    ] {
        assert!(
            !Timestamps {
                created_at: Some(origin),
                updated_at: None
            }
            .is_current()
        );
    }
    assert!(
        !Timestamps {
            created_at: Some(TimestampOrigin::SourceMetadata {
                field: TimestampField::SessionTimeUpdated,
                rule_version: VERSION,
            }),
            updated_at: None
        }
        .is_current_for(Provider::Opencode)
    );
}
