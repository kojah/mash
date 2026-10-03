//! Aider's native Markdown history, as written by aider/io.py.
use super::{Conversation, NormalizedMessage, ParsedFile};
use crate::{AppError, provider::Provider, role::Role};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Default)]
struct Turn {
    role: Option<Role>,
    content: String,
    has_lines: bool,
}

impl Turn {
    fn flush(
        &mut self,
        path: &Path,
        messages: &mut Vec<NormalizedMessage>,
    ) -> Result<(), AppError> {
        let content = std::mem::take(&mut self.content);
        self.has_lines = false;
        let Some(role) = self.role.take() else {
            return Ok(());
        };
        let content = content.trim().to_owned();
        if content.is_empty() {
            return Ok(());
        }
        let ordinal = i64::try_from(messages.len())
            .map_err(|_| AppError::internal("too many Aider messages"))?;
        let clean = crate::silver::dialogue::visible(&role, &content);
        let dialogue_text = (clean.as_ref() != content).then(|| clean.into_owned());
        messages.push(NormalizedMessage {
            id: super::parsers::stable_id(
                "message",
                &[
                    Provider::Aider.as_str(),
                    &path.to_string_lossy(),
                    &ordinal.to_string(),
                ],
            ),
            ordinal,
            context: crate::message_context::MessageContext::Linear,
            role,
            content,
            dialogue_text,
            created_at: None,
        });
        Ok(())
    }

    fn append(
        &mut self,
        role: Role,
        line: &str,
        path: &Path,
        messages: &mut Vec<NormalizedMessage>,
    ) -> Result<(), AppError> {
        if self.role.as_ref() != Some(&role) {
            self.flush(path, messages)?;
        }
        self.role = Some(role);
        self.push_line(line)
    }

    fn push_line(&mut self, line: &str) -> Result<(), AppError> {
        if self.role.is_none() {
            return Ok(());
        }
        let separator = usize::from(self.has_lines);
        if self.content.len() + separator + line.len() > super::MAX_RECORD_BYTES {
            return Err(AppError::configuration(
                "Aider turn exceeds the 8 MiB ingestion limit",
            ));
        }
        if separator > 0 {
            self.content.push('\n');
        }
        self.content.push_str(line);
        self.has_lines = true;
        Ok(())
    }
}

/// Delimiters inside assistant code fences are ordinary content. Aider prefixes
/// every user-input line with ####, and marks operational output with >.
pub(super) fn parse(path: &Path) -> Result<ParsedFile, AppError> {
    let file = File::open(path).map_err(AppError::io)?;
    let mut messages = Vec::new();
    let mut turn = Turn::default();
    let mut fence = None::<(u8, usize)>;
    let mut recognized = false;
    let mut user_continues = false;
    for line in super::records::Lines::new(BufReader::new(file)) {
        let line = line?;
        if let Some((character, length)) = fence {
            let marker = fence_marker(&line);
            turn.push_line(&line)?;
            if marker.is_some_and(|(c, n)| c == character && n >= length)
                && line.trim().bytes().all(|c| c == character)
            {
                fence = None;
            }
            continue;
        }
        if line.starts_with("# aider chat started at ") {
            turn.flush(path, &mut messages)?;
            recognized = true;
            user_continues = false;
            turn.append(Role::System, &line, path, &mut messages)?;
        } else if let Some(text) = line.strip_prefix("#### ") {
            recognized = true;
            if !user_continues {
                turn.flush(path, &mut messages)?;
            }
            turn.append(Role::User, text.trim_end(), path, &mut messages)?;
            user_continues = line.ends_with("  ");
        } else if line.trim().is_empty() {
            // A blank line after user input separates its answer; blanks inside
            // assistant Markdown remain part of the same response.
            if turn.role == Some(Role::User) {
                turn.flush(path, &mut messages)?;
            } else {
                turn.push_line("")?;
            }
            user_continues = false;
        } else if recognized {
            user_continues = false;
            let role = if line.starts_with("> ") {
                Role::Tool
            } else {
                Role::Assistant
            };
            turn.append(role.clone(), &line, path, &mut messages)?;
            if role == Role::Assistant {
                fence = fence_marker(&line);
            }
        }
    }
    turn.flush(path, &mut messages)?;
    let malformed_records = u64::from(!recognized || fence.is_some());
    if messages.is_empty() {
        return Ok(ParsedFile {
            conversation: None,
            malformed_records,
            first_record_issue: None,
        });
    }
    let title = messages
        .iter()
        .filter(|m| m.role == Role::User)
        .find_map(|m| crate::silver::dialogue::title(&m.content));
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::parsers::stable_id(
                "session",
                &[Provider::Aider.as_str(), &path.to_string_lossy()],
            ),
            provider: Provider::Aider,
            title_origin: None,
            claude_lineage: None,
            source: super::SourceIdentity::file(path),
            title,
            // The native timestamp has no offset, and a custom history path may
            // be outside the repository. Do not invent either piece of metadata.
            project: None,
            repository: None,
            created_at: None,
            updated_at: None,
            messages: super::messages::Messages::stage(messages)?,
        }),
        malformed_records,
        first_record_issue: None,
    })
}

fn fence_marker(line: &str) -> Option<(u8, usize)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let character = *trimmed.as_bytes().first()?;
    if !matches!(character, b'`' | b'~') {
        return None;
    }
    let count = trimmed.bytes().take_while(|&c| c == character).count();
    (count >= 3).then_some((character, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_turn_limit_includes_fences_and_blank_lines() {
        let mut turn = Turn {
            role: Some(Role::Assistant),
            ..Turn::default()
        };
        let half = "x".repeat(super::super::MAX_RECORD_BYTES / 2);
        turn.push_line(&half).unwrap();
        assert!(turn.push_line(&half).is_err());
        assert_eq!(turn.content, half);

        let mut turn = Turn {
            role: Some(Role::Assistant),
            content: "x".repeat(super::super::MAX_RECORD_BYTES),
            has_lines: true,
        };
        assert!(
            turn.push_line("").is_err(),
            "blank lines also consume bytes"
        );
        let mut ignored = Turn::default();
        for _ in 0..100 {
            ignored.push_line("").unwrap();
        }
        assert_eq!(ignored.content, "");
        assert!(!ignored.has_lines);

        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            format!(
                "# aider chat started at 2026-09-30\n\n#### request\n\n```\n{half}\n{half}\n```\n"
            ),
        )
        .unwrap();
        let error = parse(file.path()).err().expect("oversized fenced response");
        assert!(error.error.message.contains("Aider turn exceeds"));
    }

    #[tokio::test]
    #[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
    async fn core_indexes_searches_and_views_native_history_with_provider_filters() {
        use crate::core::{IndexRequest, Operation, Paths, Request, Response};
        let directory = tempfile::tempdir().unwrap();
        let history = directory.path().join(".aider.chat.history.md");
        std::fs::write(&history, "# aider chat started at 2026-09-30 10:00:00\n\n#### Fix the cBioPortal importer  \n\nI'll repair its mutation parsing.\n\n> privatecommandmarker  \n").unwrap();
        let config = directory.path().join("config.toml");
        std::fs::write(
            &config,
            toml::to_string(&toml::toml! {
                version = 1
                providers = { aider = { roots = [(directory.path().to_str().unwrap())] } }
                index = { all_history = true }
            })
            .unwrap(),
        )
        .unwrap();
        let paths = Paths {
            config: Some(config),
            db: Some(directory.path().join("index.sqlite3")),
            models_dir: Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into()),
        };
        assert!(matches!(
            crate::core::execute(Request {
                paths: paths.clone(),
                operation: Operation::Index(IndexRequest::default())
            })
            .await
            .unwrap(),
            Response::Index(_)
        ));
        let filters = crate::search_filter::Selection {
            provider: vec![Provider::Aider],
            ..crate::search_filter::Selection::default()
        };
        let Response::Search(response) = crate::core::execute(Request {
            paths: paths.clone(),
            operation: Operation::Search {
                fusion: crate::gold::config::FusionOverrides::default(),
                ranking: None,
                diagnostics: false,
                query: "cbioportal importer".into(),
                intent: None,
                limit: Some(5),
                unit: Some(crate::search_results::SearchUnit::Threads),
                filters: Box::new(filters),
                shared: false,
                local: true,
            },
        })
        .await
        .unwrap() else {
            panic!("search response");
        };
        let crate::search_results::Results::Threads(threads) = response.results else {
            panic!("threads");
        };
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].provider, Provider::Aider);
        assert!(
            threads[0].matches[0]
                .snippet
                .contains("cBioPortal importer")
        );
        let Response::View(view) = crate::core::execute(Request {
            paths,
            operation: Operation::View {
                id: threads[0].matches[0].message_id.clone(),
                context: 5,
                shared: None,
            },
        })
        .await
        .unwrap() else {
            panic!("view response");
        };
        assert!(
            view.messages
                .iter()
                .any(|message| message.content.contains("privatecommandmarker"))
        );
        let storage =
            crate::storage::Storage::open_existing(&directory.path().join("index.sqlite3"))
                .unwrap();
        assert_eq!(storage.counts().unwrap().searchable_messages, 2);
    }

    #[test]
    fn native_turns_preserve_code_and_operational_context_without_indexing_it() {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(concat!(
            "# aider chat started at 2026-09-30 10:00:00\r\n\r\n",
            "> added importer.py\r\n",
            "\r\n#### Fix the cBioPortal importer  \r\n#### Keep its tests  \r\n",
            "\r\nI'll update it.\r\n\r\n```markdown\r\n#### example heading\r\n> quoted example\r\n```\r\n",
            "\r\n> Applied edit to importer.py  \r\n",
            "\r\n#### Run the tests  \r\n\r\nTests passed.\r\n",
        ).as_bytes()).unwrap();
        let conversation = parse(file.path()).unwrap().conversation.unwrap();
        let users = conversation
            .messages
            .inline()
            .iter()
            .filter(|m| m.role == Role::User)
            .collect::<Vec<_>>();
        assert_eq!(users.len(), 2);
        assert_eq!(
            users[0].content,
            "Fix the cBioPortal importer\nKeep its tests"
        );
        assert_eq!(
            conversation.title.as_deref(),
            Some(users[0].content.as_str())
        );
        assert!(
            conversation
                .messages
                .inline()
                .iter()
                .any(|m| m.role == Role::Assistant
                    && m.content.contains("#### example heading\n> quoted example"))
        );
        assert!(
            conversation
                .messages
                .inline()
                .iter()
                .filter(|m| matches!(m.role, Role::Tool | Role::System))
                .all(|m| m.dialogue_text.as_deref() == Some(""))
        );
        assert!(conversation.created_at.is_none());
        let repeated = parse(file.path()).unwrap().conversation.unwrap();
        assert_eq!(conversation.id, repeated.id);
        assert_eq!(
            conversation.messages.inline()[3].id,
            repeated.messages.inline()[3].id
        );
    }

    #[test]
    fn unrelated_markdown_is_an_incomplete_source_not_an_empty_history() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            "# ordinary documentation\nDo not index this file.\n",
        )
        .unwrap();
        let result = parse(file.path()).unwrap();
        assert!(result.conversation.is_none());
        assert_eq!(result.malformed_records, 1);
    }

    #[test]
    fn discovery_selects_native_names_and_explicit_custom_files() {
        let directory = tempfile::tempdir().unwrap();
        let native = directory.path().join(".aider.chat.history.md");
        let custom = directory.path().join("custom.md");
        for path in [&native, &custom, &directory.path().join("README.md")] {
            std::fs::write(path, "placeholder").unwrap();
        }
        let options = super::super::IndexOptions {
            providers: [(Provider::Aider, vec![directory.path().to_path_buf()])].into(),
            roots_are_authoritative: true,
            since_days: None,
        };
        let scans = super::super::direct_scans(&options).unwrap();
        assert_eq!(
            scans[0]
                .sources
                .iter()
                .map(|source| source.identity.clone())
                .collect::<Vec<_>>(),
            [super::super::SourceIdentity::file(&native)]
        );
        let options = super::super::IndexOptions {
            providers: [(Provider::Aider, vec![custom.clone()])].into(),
            ..options
        };
        assert_eq!(
            super::super::direct_scans(&options).unwrap()[0]
                .sources
                .iter()
                .map(|source| source.identity.clone())
                .collect::<Vec<_>>(),
            [super::super::SourceIdentity::file(custom)]
        );
        std::fs::remove_file(native).unwrap();
        assert!(scans[0].verify().is_err());
    }

    #[test]
    fn unclosed_code_fences_do_not_create_turns_or_complete_checkpoints() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "# aider chat started at 2026-09-30 10:00:00\n\n#### example  \n\n````markdown\n#### code heading\n# aider chat started at 2020-01-01 10:00:00\n```\n").unwrap();
        let result = parse(file.path()).unwrap();
        assert_eq!(result.malformed_records, 1);
        let messages = result.conversation.unwrap().messages;
        assert_eq!(
            messages
                .inline()
                .iter()
                .filter(|m| m.role == Role::User)
                .count(),
            1
        );
        assert!(
            messages
                .inline()
                .last()
                .unwrap()
                .content
                .contains("2020-01-01")
        );
    }
}
