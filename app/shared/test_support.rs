//! A seeded local index for tests of shared search, here and in the crates above.
use crate::silver::{Chat, NormalizedMessage};
use crate::storage::Storage;
use std::path::Path;

/// A certified seeded index with ready local views.
pub async fn seed_ready(path: &Path) -> crate::views::local::test_support::Fixture {
    seed_certified(path);
    crate::views::local::test_support::Fixture::new(path, "local-node").await
}

/// The seeded index with its silver scan certified complete.
pub fn seed_certified(path: &Path) {
    drop(seed(path));
    let mut writer = Storage::open_writer(path).unwrap();
    writer
        .begin_silver_scan(crate::provider::Provider::Codex)
        .unwrap();
    writer
        .finish_silver_scan(crate::provider::Provider::Codex, true)
        .unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.commit_writer().unwrap();
}

/// An index holding one chat: a searchable user message and a tool message.
pub fn seed(path: &Path) -> Storage {
    let mut storage = Storage::open_writer(path).expect("writer");
    storage
        .replace_chat(&Chat {
            id: "conversation".to_owned(),
            provider: crate::provider::Provider::Codex,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: crate::silver::SourceIdentity::file("/tmp/shared-fixture.jsonl"),
            project: None,
            repository: None,
            title: Some("fixture".to_owned()),
            created_at: Some(1),
            updated_at: Some(2),
            messages: vec![
                NormalizedMessage {
                    attachments: Vec::new(),
                    id: "searchable".to_owned(),
                    ordinal: 0,
                    context: crate::message_context::MessageContext::Linear,
                    role: crate::role::Role::User,
                    content: "hello shared history".to_owned(),
                    dialogue_text: None,
                    created_at: Some(1),
                    tools: crate::tools::Activity::default(),
                },
                NormalizedMessage {
                    attachments: Vec::new(),
                    id: "context".to_owned(),
                    ordinal: 1,
                    context: crate::message_context::MessageContext::Linear,
                    role: crate::role::Role::Tool,
                    content: "hello raw tool output".to_owned(),
                    dialogue_text: Some(String::new()),
                    created_at: Some(2),
                    tools: crate::tools::Activity::default(),
                },
            ]
            .into(),
        })
        .expect("conversation");
    storage.commit_writer().expect("commit");
    storage
}
