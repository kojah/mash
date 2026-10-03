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
    #[serde(rename = "git_repo_root")]
    GitRepoRoot,
    #[serde(rename = "session.title")]
    SessionTitle,
    #[serde(rename = "session.directory")]
    SessionDirectory,
    #[serde(rename = "project.worktree")]
    ProjectWorktree,
}

// Only unconditional native field selections belong here. Native-title versus
// message-summary fallbacks require adapter-level evidence, not value equality.
fn native_fields(provider: Provider) -> [Option<SourceField>; 3] {
    use SourceField::{Cwd, GitRepoRoot, ProjectWorktree, SessionDirectory, SessionTitle};
    match provider {
        Provider::Codex | Provider::ClaudeCode | Provider::Pi => [None, Some(Cwd), None],
        Provider::Hermes => [None, Some(Cwd), Some(GitRepoRoot)],
        Provider::Opencode => [
            Some(SessionTitle),
            Some(SessionDirectory),
            Some(ProjectWorktree),
        ],
        _ => [None; 3],
    }
}

impl Evidence {
    pub(crate) fn is_current(&self) -> bool {
        self.version == VERSION && [&self.title, &self.project, &self.repository].into_iter().all(|origin| {
            !matches!(origin, Some(Origin::FirstVisibleUserMessage { message_id, rule_version }) if message_id.is_empty() || *rule_version != VERSION)
        })
    }

    /// Source checkpoint freshness includes the audited native origins. Old
    /// unknown evidence stays readable for source-free gold builds and viewing.
    pub(crate) fn is_current_for(&self, provider: Provider) -> bool {
        self.is_current()
            && [&self.title, &self.project, &self.repository]
                .into_iter()
                .zip(native_fields(provider))
                .all(|(origin, field)| {
                    field.is_none_or(|expected| {
                        origin.as_ref().is_none_or(|origin| {
                            matches!(origin, Origin::SourceMetadata { field } if *field == expected)
                        })
                    })
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
        }
        for (origin, field) in [
            &mut evidence.title,
            &mut evidence.project,
            &mut evidence.repository,
        ]
        .into_iter()
        .zip(native_fields(conversation.provider))
        {
            if let (Some(origin), Some(field)) = (origin.as_mut(), field) {
                *origin = Origin::SourceMetadata { field };
            }
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
