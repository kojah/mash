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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) timestamps: Option<Timestamps>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Timestamps {
    pub(crate) created_at: Option<TimestampOrigin>,
    pub(crate) updated_at: Option<TimestampOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum TimestampOrigin {
    Unknown,
    SourceMetadata {
        field: TimestampField,
        rule_version: u32,
    },
    MessageBoundary {
        message_id: String,
        selection: Boundary,
        rule_version: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Boundary {
    Minimum,
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TimestampField {
    #[serde(rename = "session.time_created")]
    SessionTimeCreated,
    #[serde(rename = "session.time_updated")]
    SessionTimeUpdated,
    #[serde(rename = "threads.created_at_ms")]
    ThreadCreatedAtMs,
    #[serde(rename = "threads.updated_at_ms")]
    ThreadUpdatedAtMs,
    #[serde(rename = "threads.created_at")]
    ThreadCreatedAt,
    #[serde(rename = "threads.updated_at")]
    ThreadUpdatedAt,
}

impl Timestamps {
    pub(crate) fn matches_values(&self, created_at: Option<i64>, updated_at: Option<i64>) -> bool {
        self.created_at.is_some() == created_at.is_some()
            && self.updated_at.is_some() == updated_at.is_some()
    }

    fn validate_selection(&self, conversation: &Conversation) -> Result<(), AppError> {
        if !self.matches_values(conversation.created_at, conversation.updated_at)
            || !self.is_current_for(conversation.provider)
        {
            return Err(AppError::internal(
                "timestamp origin does not match selected metadata",
            ));
        }
        Ok(())
    }

    fn is_current(&self) -> bool {
        [&self.created_at, &self.updated_at]
            .into_iter()
            .all(|origin| match origin {
                Some(TimestampOrigin::SourceMetadata { rule_version, .. }) => {
                    *rule_version == VERSION
                }
                Some(TimestampOrigin::MessageBoundary {
                    message_id,
                    rule_version,
                    ..
                }) => *rule_version == VERSION && !message_id.is_empty(),
                _ => true,
            })
    }

    fn is_current_for(&self, provider: Provider) -> bool {
        [&self.created_at, &self.updated_at]
            .into_iter()
            .zip([
                (Boundary::Minimum, TimestampField::SessionTimeCreated),
                (Boundary::Maximum, TimestampField::SessionTimeUpdated),
            ])
            .all(|(origin, (boundary, expected))| {
                origin.as_ref().is_none_or(|origin| match provider {
                    Provider::Codex => match origin {
                        TimestampOrigin::MessageBoundary { selection, .. } => {
                            *selection == boundary
                        }
                        TimestampOrigin::SourceMetadata { field, .. } => matches!(
                            (boundary, field),
                            (
                                Boundary::Minimum,
                                TimestampField::ThreadCreatedAt | TimestampField::ThreadCreatedAtMs
                            ) | (
                                Boundary::Maximum,
                                TimestampField::ThreadUpdatedAt | TimestampField::ThreadUpdatedAtMs
                            )
                        ),
                        TimestampOrigin::Unknown => false,
                    },
                    Provider::ClaudeCode => matches!(origin,
                    TimestampOrigin::MessageBoundary { selection, .. } if *selection == boundary),
                    Provider::Opencode => matches!(origin,
                    TimestampOrigin::SourceMetadata { field, .. } if *field == expected),
                    _ => true,
                })
            })
    }
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
    #[serde(rename = "session.title")]
    SessionTitle,
    #[serde(rename = "threads.title")]
    ThreadTitle,
    #[serde(rename = "session.directory")]
    SessionDirectory,
    #[serde(rename = "project.worktree")]
    ProjectWorktree,
}

// Only unconditional native field selections belong here. Native-title versus
// message-summary fallbacks require adapter-level evidence, not value equality.
fn native_fields(provider: Provider) -> [Option<SourceField>; 3] {
    use SourceField::{Cwd, ProjectWorktree, SessionDirectory, SessionTitle};
    match provider {
        Provider::Codex | Provider::ClaudeCode => [None, Some(Cwd), None],
        Provider::Opencode => [
            Some(SessionTitle),
            Some(SessionDirectory),
            Some(ProjectWorktree),
        ],
        _ => [None; 3],
    }
}

impl Evidence {
    fn validate_selected_metadata(&self, conversation: &Conversation) -> Result<(), AppError> {
        if self.is_current()
            && self.matches_metadata_values(
                conversation.title.as_deref(),
                conversation.project.as_deref(),
                conversation.repository.as_deref(),
                conversation.created_at,
                conversation.updated_at,
            )
        {
            Ok(())
        } else {
            Err(AppError::internal("invalid adapter metadata evidence"))
        }
    }

    pub(crate) fn matches_metadata_values(
        &self,
        title: Option<&str>,
        project: Option<&str>,
        repository: Option<&str>,
        created_at: Option<i64>,
        updated_at: Option<i64>,
    ) -> bool {
        [&self.title, &self.project, &self.repository]
            .into_iter()
            .zip([title.is_some(), project.is_some(), repository.is_some()])
            .all(|(origin, present)| origin.is_some() == present)
            && self
                .timestamps
                .as_ref()
                .is_none_or(|times| times.matches_values(created_at, updated_at))
    }

    pub(crate) fn is_current(&self) -> bool {
        self.version == VERSION
            && self.timestamps.as_ref().is_none_or(Timestamps::is_current)
            && [&self.title, &self.project, &self.repository]
                .into_iter()
                .all(|origin| match origin {
                    Some(Origin::FirstVisibleUserMessage {
                        message_id,
                        rule_version,
                    }) => !message_id.is_empty() && *rule_version == VERSION,
                    _ => true,
                })
    }

    /// Source checkpoint freshness includes the audited native origins. Old
    /// unknown evidence stays readable for source-free views builds and viewing.
    pub(crate) fn is_current_for(&self, provider: Provider) -> bool {
        self.is_current()
            && match self.timestamps.as_ref() {
                Some(times) => times.is_current_for(provider),
                None => !matches!(
                    provider,
                    Provider::Codex | Provider::ClaudeCode | Provider::Opencode
                ),
            }
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
            timestamps: Some(Timestamps {
                created_at: conversation.created_at.map(|_| TimestampOrigin::Unknown),
                updated_at: conversation.updated_at.map(|_| TimestampOrigin::Unknown),
            }),
        };
        // These adapters always select the summary title and native cwd. Do not
        // apply value-equality heuristics to adapters with native-title fallbacks.
        if matches!(
            conversation.provider,
            Provider::Codex | Provider::ClaudeCode
        ) {
            let summary = conversation.messages.summary()?;
            let times = evidence
                .timestamps
                .as_mut()
                .ok_or_else(|| AppError::internal("missing captured timestamps"))?;
            for (origin, selected, boundary_time, message_id, selection) in [
                (
                    &mut times.created_at,
                    conversation.created_at,
                    summary.created_at,
                    summary.created_message_id,
                    Boundary::Minimum,
                ),
                (
                    &mut times.updated_at,
                    conversation.updated_at,
                    summary.updated_at,
                    summary.updated_message_id,
                    Boundary::Maximum,
                ),
            ] {
                if selected == boundary_time
                    && let Some(message_id) = message_id
                {
                    *origin = Some(TimestampOrigin::MessageBoundary {
                        message_id,
                        selection,
                        rule_version: VERSION,
                    });
                }
            }
            if conversation.title == summary.title
                && let Some(message_id) = summary.title_message_id
            {
                evidence.title = Some(Origin::FirstVisibleUserMessage {
                    message_id,
                    rule_version: VERSION,
                });
            }
        }
        if conversation.provider == Provider::Opencode
            && let Some(times) = evidence.timestamps.as_mut()
        {
            for (origin, field) in [
                (&mut times.created_at, TimestampField::SessionTimeCreated),
                (&mut times.updated_at, TimestampField::SessionTimeUpdated),
            ] {
                if origin.is_some() {
                    *origin = Some(TimestampOrigin::SourceMetadata {
                        field,
                        rule_version: VERSION,
                    });
                }
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
        if let Some(origin) = &conversation.title_origin {
            if conversation.title.is_none() {
                return Err(AppError::internal("title origin has no selected title"));
            }
            evidence.title = Some(origin.clone());
        }
        if let Some(times) = &conversation.timestamp_origins {
            times.validate_selection(conversation)?;
            evidence.timestamps = Some(times.clone());
        }
        evidence.validate_selected_metadata(conversation)?;
        Ok(evidence)
    }
}

#[cfg(test)]
#[path = "enrichment_timestamps_tests.rs"]
mod timestamp_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_inferred_absent_and_unknown_metadata_remain_distinct() {
        let mut thread = Conversation {
            id: "thread".into(),
            provider: Provider::Codex,
            source: super::super::SourceIdentity::file("fixture"),
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            title: Some("qdrant filtering".into()),
            project: Some("/work/project".into()),
            repository: Some("https://example.com/repo".into()),
            created_at: None,
            updated_at: None,
            messages: vec![super::super::NormalizedMessage {
                attachments: Vec::new(),
                id: "first".into(),
                ordinal: 0,
                role: crate::role::Role::User,
                context: crate::message_context::MessageContext::Linear,
                content: "qdrant filtering".into(),
                dialogue_text: None,
                created_at: None,
                tools: crate::tools::Activity::default(),
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
        thread.provider = Provider::Cursor;
        assert_eq!(
            Evidence::capture(&thread).unwrap().title,
            Some(Origin::Unknown)
        );
        thread.title = None;
        assert!(Evidence::capture(&thread).unwrap().title.is_none());
        assert!(serde_json::from_str::<Evidence>("{}").is_err());
    }
}
