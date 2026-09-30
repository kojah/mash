//! Searchable conversation text; raw provider records remain available through view.
use crate::role::Role;
use std::borrow::Cow;

const CONTEXT_TAGS: &[&str] = &[
    "system-reminder",
    "environment_context",
    "permissions",
    "collaboration_mode",
    "skills_instructions",
    "recommended_plugins",
    "multi_agent_role",
    "multi_agent_mode",
    "in-app-browser-context",
];

/// Remove known agent-injected context without matching ordinary mentions of it.
pub(crate) fn project<'a>(role: &Role, text: &'a str) -> Cow<'a, str> {
    if !matches!(role, Role::User | Role::Assistant) {
        return Cow::Borrowed("");
    }
    // Skill bodies have no reliable closing delimiter. These are dedicated
    // injected messages, not arbitrary references to skills in ordinary prose.
    let trimmed = text.trim_start();
    if trimmed.starts_with("Base directory for this skill:")
        || trimmed.starts_with(
            "The following is the Codex agent history added since your last approval assessment.",
        )
        || trimmed.starts_with(
            "This session is being continued from a previous conversation that ran out of context.",
        )
        || trimmed.starts_with("This conversation is a continuation of an earlier chat.")
    {
        return Cow::Borrowed("");
    }
    let mut result = Cow::Borrowed(text);
    for tag in CONTEXT_TAGS {
        result = remove_blocks(result, tag);
    }
    if trimmed.starts_with("# AGENTS.md instructions") && text.contains("<INSTRUCTIONS>") {
        result = remove_blocks(result, "INSTRUCTIONS");
        let content = result.as_ref();
        let end = content.find('\n').unwrap_or(content.len());
        result = Cow::Owned(content[end..].trim_start().to_owned());
    }
    result
}

fn remove_blocks<'a>(text: Cow<'a, str>, tag: &str) -> Cow<'a, str> {
    let opening = format!("<{tag}");
    let closing = format!("</{tag}>");
    let mut cursor = 0;
    let mut copied = 0;
    let mut output = None::<String>;
    while let Some(relative) = text[cursor..].find(&opening) {
        let start = cursor + relative;
        let boundary = start + opening.len();
        // Markdown examples are user-authored evidence, not injected context.
        if text[..start].bytes().filter(|b| *b == b'`').count() % 2 != 0 {
            cursor = boundary;
            continue;
        }
        // Attribute-bearing context blocks are allowed; longer tag names aren't.
        if !text[boundary..].starts_with(['>', ' ', '\n', '\t']) {
            cursor = boundary;
            continue;
        }
        let ending = text[boundary..]
            .find(&closing)
            .map(|end| boundary + end + closing.len());
        let ending = ending.or_else(|| {
            let opening_end = text[boundary..].find('>')? + boundary;
            let labeled_close = format!("</{}>", &text[start + 1..opening_end]);
            text[opening_end..]
                .find(&labeled_close)
                .map(|end| opening_end + end + labeled_close.len())
        });
        let Some(end) = ending else {
            let line = text[..start].rsplit('\n').next().unwrap_or("");
            if !line.trim().is_empty() {
                cursor = boundary;
                continue;
            }
            // Incomplete serialized context is still context, rather than query evidence.
            output
                .get_or_insert_with(String::new)
                .push_str(&text[copied..start]);
            cursor = text.len();
            copied = cursor;
            break;
        };
        output
            .get_or_insert_with(String::new)
            .push_str(&text[copied..start]);
        cursor = end;
        copied = end;
    }
    if let Some(mut output) = output {
        output.push_str(&text[copied..]);
        Cow::Owned(output.trim().to_owned())
    } else {
        text
    }
}

pub(crate) fn title(text: &str) -> Option<String> {
    // Older indexes may have truncated an injected header before its delimiter.
    if text.trim_start().starts_with("# AGENTS.md instructions") {
        return None;
    }
    let clean = project(&Role::User, text);
    let clean = clean.trim();
    (!clean.is_empty()).then(|| clean.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedicated_review_transcripts_and_compaction_summaries_are_not_user_evidence() {
        for text in [
            "The following is the Codex agent history added since your last approval assessment.\nsearch authentication",
            "This session is being continued from a previous conversation that ran out of context.\nredis",
            "This conversation is a continuation of an earlier chat.\ncbioportal",
            "<in-app-browser-context>automatic state</in-app-browser-context>",
        ] {
            assert_eq!(project(&Role::User, text).trim(), "");
        }
        let ordinary = "Can we remove automatic approval-review transcripts from search?";
        assert_eq!(project(&Role::User, ordinary), ordinary);
    }

    #[test]
    fn removes_context_preserves_requests_and_ordinary_mentions() {
        let input = "# AGENTS.md instructions for /work\n<INSTRUCTIONS>cbioportal rules</INSTRUCTIONS><environment_context>cwd</environment_context>\nfix the importer";
        assert_eq!(project(&Role::User, input), "fix the importer");
        assert_eq!(
            project(
                &Role::User,
                "<system-reminder>cbioportal skill</system-reminder>review PR 180"
            ),
            "review PR 180"
        );
        assert_eq!(
            project(
                &Role::User,
                "Base directory for this skill: /skills/cbioportal\nsecret instructions"
            ),
            ""
        );
        assert_eq!(
            project(
                &Role::User,
                "How do I edit AGENTS.md or write a system-reminder?"
            ),
            "How do I edit AGENTS.md or write a system-reminder?"
        );
    }

    #[test]
    fn handles_repeated_and_incomplete_blocks_without_overmatching_tags() {
        assert_eq!(
            project(
                &Role::User,
                "A<system-reminder>x</system-reminder>B<system-reminder>y</system-reminder>C"
            ),
            "ABC"
        );
        assert_eq!(
            project(&Role::User, "ask\n<system-reminder>unfinished"),
            "ask"
        );
        assert_eq!(
            project(
                &Role::User,
                "<permissions-extra>example</permissions-extra>"
            ),
            "<permissions-extra>example</permissions-extra>"
        );
        assert_eq!(
            project(
                &Role::User,
                "<permissions-extra>x</permissions-extra><permissions instructions>noise</permissions instructions>"
            ),
            "<permissions-extra>x</permissions-extra>"
        );
    }

    #[test]
    fn context_roles_are_unsearchable_and_titles_use_real_text() {
        assert_eq!(title("# AGENTS.md instructions for /work"), None);
        for role in [
            Role::System,
            Role::Developer,
            Role::Tool,
            Role::Other("toolResult".into()),
        ] {
            assert_eq!(project(&role, "cbioportal"), "");
        }
        assert_eq!(
            title("<recommended_plugins>list</recommended_plugins>"),
            None
        );
        assert_eq!(
            title("<system-reminder>noise</system-reminder>cBioPortal importer"),
            Some("cBioPortal importer".into())
        );
    }

    #[test]
    fn preserves_literal_examples_and_inline_unclosed_tags() {
        for input in [
            "what does `<system-reminder>example</system-reminder>` mean?",
            "```xml\n<system-reminder>example</system-reminder>\n```",
            "what does <system-reminder> mean?",
            "<INSTRUCTIONS>user XML</INSTRUCTIONS>",
        ] {
            assert_eq!(project(&Role::User, input), input);
        }
    }
}
