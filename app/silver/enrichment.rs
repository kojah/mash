//! Versioned evidence for canonical metadata, separate from retrieval policy.
use super::Conversation;
use crate::{AppError, provider::Provider};
use serde::{Deserialize, Serialize};

pub(crate) const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Evidence {
    pub(crate) version: u32,
    pub(crate) title: Option<Origin>,
    pub(crate) project: Option<Origin>,
    pub(crate) repository: Option<Origin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Origin {
    Unknown,
    SourceMetadata {
        field: SourceField,
    },
    FirstVisibleUserMessage {
        message_id: String,
        rule_version: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SourceField {
    Cwd,
}

impl Evidence {
    pub(crate) fn is_current(&self) -> bool {
        self.version == VERSION && [&self.title, &self.project, &self.repository].into_iter().all(|origin| {
            !matches!(origin, Some(Origin::FirstVisibleUserMessage { message_id, rule_version }) if message_id.is_empty() || *rule_version != VERSION)
        })
    }

    pub(crate) fn capture(conversation: &Conversation) -> Result<Self, AppError> {
        let mut evidence = Self {
            version: VERSION,
            title: conversation.title.as_ref().map(|_| Origin::Unknown),
            project: conversation.project.as_ref().map(|_| Origin::Unknown),
            repository: conversation.repository.as_ref().map(|_| Origin::Unknown),
        };
        // These adapters always select the summary title and native cwd. Do not
        // apply value-equality heuristics to adapters with native-title fallbacks.
        if matches!(
            conversation.provider,
            Provider::Codex | Provider::ClaudeCode
        ) {
            let summary = conversation.messages.summary()?;
            if conversation.title == summary.title
                && let Some(message_id) = summary.title_message_id
            {
                evidence.title = Some(Origin::FirstVisibleUserMessage {
                    message_id,
                    rule_version: VERSION,
                });
            }
            evidence.project = conversation
                .project
                .as_ref()
                .map(|_| Origin::SourceMetadata {
                    field: SourceField::Cwd,
                });
        }
        Ok(evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_inferred_absent_and_unknown_metadata_remain_distinct() {
        let mut thread = Conversation {
            id: "thread".into(),
            provider: Provider::Codex,
            source: super::super::SourceIdentity::file("fixture"),
            claude_lineage: None,
            title: Some("qdrant filtering".into()),
            project: Some("/work/project".into()),
            repository: Some("https://example.com/repo".into()),
            created_at: None,
            updated_at: None,
            messages: vec![super::super::NormalizedMessage {
                id: "first".into(),
                ordinal: 0,
                role: crate::role::Role::User,
                context: crate::message_context::MessageContext::Linear,
                content: "qdrant filtering".into(),
                dialogue_text: None,
                created_at: None,
            }]
            .into(),
        };
        for provider in [Provider::Codex, Provider::ClaudeCode] {
            thread.provider = provider;
            let evidence = Evidence::capture(&thread).unwrap();
            assert_eq!(
                evidence.title,
                Some(Origin::FirstVisibleUserMessage {
                    message_id: "first".into(),
                    rule_version: VERSION
                })
            );
            assert_eq!(
                evidence.project,
                Some(Origin::SourceMetadata {
                    field: SourceField::Cwd
                })
            );
            assert_eq!(evidence.repository, Some(Origin::Unknown));
        }
        // A native-title adapter can produce the same text by a different path.
        thread.provider = Provider::Hermes;
        assert_eq!(
            Evidence::capture(&thread).unwrap().title,
            Some(Origin::Unknown)
        );
        thread.title = None;
        assert!(Evidence::capture(&thread).unwrap().title.is_none());
        assert!(serde_json::from_str::<Evidence>("{}").is_err());
    }
}
