//! Citing an agent's own session by quote, before and after it is indexed.
use super::*;
use crate::records::live;

fn by_session(session: &str, quote: &str, purpose: EvidenceUse) -> Citation {
    Citation {
        session_id: Some(session.into()),
        message_id: String::new(),
        quote: quote.into(),
        purpose,
        expected_fingerprint: None,
    }
}

fn preference(operation: &str, review: Review, evidence: Vec<Citation>) -> Mutation {
    Mutation {
        operation_id: operation.into(),
        change: Change::Create {
            scope: Scope::Repository {
                identity: "repo".into(),
            },
            body: Body {
                statement: "Indent with tabs".into(),
                review,
                ..body(Details::Preference {
                    state: PreferenceState::Active,
                    conditions: vec![],
                })
            },
            evidence,
            on_duplicate: records::DuplicatePolicy::Report,
        },
    }
}

fn transcript(repository: &str) -> Vec<live::Message> {
    vec![live::Message {
        session_id: "running".into(),
        quote: "tabs, not spaces".into(),
        chat_id: "running".into(),
        id: "said".into(),
        role: crate::role::Role::User,
        content: "Use tabs, not spaces.".into(),
        project: None,
        repository: Some(repository.into()),
    }]
}

#[test]
fn session_citation_selects_the_latest_retained_message_with_the_quote() {
    let (_directory, mut storage) = database();
    let created = storage
        .mutate_record(&preference(
            "retained",
            Review::Established,
            vec![
                by_session("chat", "Qdrant", EvidenceUse::Context),
                by_session("chat", "hoose Qdrant", EvidenceUse::Endorsement),
            ],
        ))
        .unwrap();
    let record = storage.record(&created.record_id, false).unwrap();
    let cited: Vec<&str> = record.revisions[0]
        .evidence
        .iter()
        .map(|source| source.message_id.as_str())
        .collect();
    assert_eq!(cited, ["again", "again"]);
    assert!(record.revisions[0].evidence_current);

    // Only the assistant said this, so it cannot be the person's endorsement.
    let error = storage
        .mutate_record(&preference(
            "assistant-words",
            Review::Established,
            vec![by_session(
                "chat",
                "for local search",
                EvidenceUse::Endorsement,
            )],
        ))
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Conflict);
    assert!(error.error.message.contains("person's messages"));
}

#[test]
fn live_quote_is_recorded_now_and_reads_current_once_indexed() {
    let (_directory, mut storage) = database();
    let request = preference(
        "live",
        Review::Established,
        vec![by_session(
            "running",
            "tabs, not spaces",
            EvidenceUse::Endorsement,
        )],
    );
    // Without the transcript there is nothing to verify the quote against.
    assert_eq!(
        storage.mutate_record(&request).unwrap_err().error.kind,
        crate::ErrorKind::Conflict
    );
    // The transcript's own project must match the record's scope.
    assert!(
        storage
            .mutate_record_citing(&request, &transcript("other-repo"))
            .unwrap_err()
            .error
            .message
            .contains("outside the record scope")
    );
    let created = storage
        .mutate_record_citing(&request, &transcript("repo"))
        .unwrap();
    let record = storage.record(&created.record_id, false).unwrap();
    let source = &record.revisions[0].evidence[0];
    assert_eq!(
        (source.chat_id.as_str(), source.message_id.as_str()),
        ("running", "said")
    );
    assert_eq!(source.source_role, Some(crate::role::Role::User));
    let current = |storage: &Storage| {
        storage.record(&created.record_id, false).unwrap().revisions[0].evidence_current
    };
    assert!(
        current(&storage),
        "verified at save; awaiting its first index"
    );
    assert_eq!(
        storage
            .recall_records(
                &records::recall::Request {
                    scope: Scope::Repository {
                        identity: "repo".into()
                    },
                    query: Some("tabs".into()),
                    file: None,
                    include_personal: false,
                    include_review: false,
                    limit: None,
                    byte_budget: None,
                },
                crate::redaction::Policy::ON
            )
            .unwrap()
            .records
            .len(),
        1,
        "a memory saved mid-session is recalled before any index runs"
    );

    // History retained only up to before the record still awaits the message.
    storage
        .connection
        .execute(
            "INSERT INTO chats(id,provider,repository,updated_at) VALUES('running','claude-code','repo',1)",
            [],
        )
        .unwrap();
    assert!(current(&storage));
    // History retained past the record without the message: the evidence is stale.
    storage
        .connection
        .execute(
            "UPDATE chats SET updated_at=9000000000000 WHERE id='running'",
            [],
        )
        .unwrap();
    assert!(!current(&storage));
    storage.connection.execute("INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('said','running',0,'user','Use tabs, not spaces.')", []).unwrap();
    assert!(
        current(&storage),
        "indexing the same message keeps it current"
    );
    // A forgotten chat never comes back, so its evidence is not awaited.
    storage.connection.execute_batch("DELETE FROM messages WHERE id='said'; DELETE FROM chats WHERE id='running'; INSERT INTO tombstones(provider,chat_id,forgotten_at) VALUES('claude-code','running',1);").unwrap();
    assert!(!current(&storage));
    storage
        .connection
        .execute("DELETE FROM tombstones", [])
        .unwrap();
    // A retry returns the stored receipt without rereading the transcript.
    assert_eq!(
        storage.mutate_record(&request).unwrap().record_id,
        created.record_id
    );
}

#[test]
fn citation_needs_exactly_one_of_message_and_session() {
    let (_directory, mut storage) = database();
    let mut both = by_session("chat", "yes", EvidenceUse::Endorsement);
    both.message_id = "yes".into();
    let mut neither = both.clone();
    neither.session_id = None;
    neither.message_id = String::new();
    for (operation, citation) in [("both", both), ("neither", neither)] {
        let error = storage
            .mutate_record(&preference(operation, Review::Candidate, vec![citation]))
            .unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::Usage, "{operation}");
    }
}

#[test]
fn live_transcript_yields_the_latest_matching_message_per_quote() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects/-work-app");
    std::fs::create_dir_all(&project).unwrap();
    let line = |id: &str, role: &str, text: &str| {
        serde_json::json!({"type":role,"sessionId":"live-1","uuid":id,"message":{"role":role,"content":text}}).to_string()
    };
    std::fs::write(
        project.join("live-1.jsonl"),
        [
            line("u1", "user", "Please use tabs, not spaces."),
            line("a1", "assistant", "Understood: tabs, not spaces."),
            line("u2", "user", "Also keep lines under 100 columns."),
        ]
        .join("\n")
            + "\n",
    )
    .unwrap();
    let roots = crate::provider::ProviderRoots::from([(
        crate::provider::Provider::ClaudeCode,
        vec![directory.path().join("projects")],
    )]);
    let quote = |session: &str, quote: &str, user_only| live::Quote {
        session_id: session.into(),
        text: quote.into(),
        user_only,
    };
    let found = live::read(
        &roots,
        &[
            quote("live-1", "tabs, not spaces", true),
            quote("live-1", "tabs, not spaces", false),
            quote("live-1", "never said", false),
            quote("../live-1", "tabs", false),
            quote("absent", "tabs", false),
        ],
    )
    .unwrap();
    let cited: Vec<(&str, &str)> = found
        .iter()
        .map(|message| (message.id.as_str(), message.role.as_str()))
        .collect();
    assert_eq!(cited.len(), 2, "{cited:?}");
    assert_eq!(cited[0].1, "user");
    assert_eq!(cited[1].1, "assistant");
    assert!(found.iter().all(|message| message.chat_id == "live-1"));
    assert!(found[0].content.contains("Please use tabs"));
}
