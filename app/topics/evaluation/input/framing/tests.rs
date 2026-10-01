use super::super::{Projection, project as topic_projection};
use super::*;

const CONTEXT: &str = "<environment_context>
  <cwd>/example/project</cwd>
  <shell>bash</shell>
  <current_date>2026-10-01</current_date>
  <timezone>Europe/Berlin</timezone>
  <filesystem><workspace_roots><root>/example/project</root></workspace_roots><permission_profile type=\"disabled\"><file_system type=\"unrestricted\" /></permission_profile></filesystem>
</environment_context>";

#[test]
fn verified_instruction_and_environment_records_leave_only_trailing_requests() {
    for header in [
        "# AGENTS.md instructions",
        "# AGENTS.md instructions for /example",
    ] {
        let envelope = format!("{header}\n\n<INSTRUCTIONS>Use Git.</INSTRUCTIONS>\n{CONTEXT}");
        assert!(matches!(topic_projection(&envelope), Projection::Omit));
        let mixed = format!("{envelope}\nInvestigate cBioPortal filtering.");
        assert!(
            matches!(topic_projection(&mixed), Projection::Replace(text) if text == "Investigate cBioPortal filtering.")
        );
    }
    let without_optional = CONTEXT
        .replace("  <cwd>/example/project</cwd>\n", "")
        .replace("  <shell>bash</shell>\n", "");
    assert_eq!(environment::strip(&without_optional), Some(""));
}

#[test]
fn unknown_environment_fields_and_values_stay_available_as_evidence() {
    for context in [
        CONTEXT.replace("<cwd>", "<unknown>"),
        CONTEXT.replace("<shell>bash</shell>", "<shell>explain PostgreSQL</shell>"),
        CONTEXT.replace(
            "<timezone>Europe/Berlin</timezone>",
            "<timezone>Discuss SQL</timezone>",
        ),
        CONTEXT.replace("2026-10-01", "not a date"),
        CONTEXT.replace("</environment_context>", ""),
        format!("{CONTEXT}</environment_context>"),
        format!("{CONTEXT}{CONTEXT}"),
        CONTEXT.replace("type=\"unrestricted\"", "type=\"custom\""),
        CONTEXT.replace(
            "<root>/example/project</root>",
            "<root>Describe Qdrant</root>",
        ),
        CONTEXT.replace(
            "<shell>bash</shell>",
            "<shell>bash</shell><shell>zsh</shell>",
        ),
        CONTEXT.replace(
            "</environment_context>",
            "<request>Investigate Redis</request></environment_context>",
        ),
    ] {
        assert!(environment::strip(&context).is_none());
        let envelope =
            format!("# AGENTS.md instructions\n<INSTRUCTIONS>Git.</INSTRUCTIONS>\n{context}");
        assert_eq!(project(&envelope).unwrap(), context.trim());
    }
}

#[test]
fn pasted_attachment_descriptions_and_user_requests_survive_without_paths() {
    let text = "# Files pasted by the user:\n\n## \"Reverse the burden of proof\": /example/pasted-text.txt\n\n## My request:\nNarrow goroutineownership and cancellationownership.";
    assert_eq!(
        project(text).unwrap(),
        "Reverse the burden of proof\n\nNarrow goroutineownership and cancellationownership."
    );
    let two = text.replace(
        "\n\n## My request:",
        "\n## \"Preserve caller ownership\": /example/other.txt\n\n## My request:",
    );
    assert!(
        project(&two)
            .unwrap()
            .starts_with("Reverse the burden of proof\nPreserve caller ownership\n\nNarrow")
    );
    assert_eq!(project("# Files pasted by the user:\n## \"Diagnose database timeout\": /example/input.txt\n## My request:\n").unwrap(), "Diagnose database timeout");
}

#[test]
fn malformed_quoted_nested_unknown_and_oversized_frames_are_preserved() {
    let agents = "# AGENTS.md instructions\n<INSTRUCTIONS>Git.</INSTRUCTIONS>";
    let files = "# Files pasted by the user:\n## \"SQL timeout\": /example/paste.txt\n## My request:\nExplain the deadlock.";
    for text in [
        format!("```\n{agents}\n```"),
        format!("```\n{files}\n```"),
        agents.replace("Git.", "<INSTRUCTIONS>nested</INSTRUCTIONS>"),
        agents.replace("</INSTRUCTIONS>", ""),
        format!("{agents}</INSTRUCTIONS>"),
        agents.replace(
            "# AGENTS.md instructions",
            "# AGENTS.md instructions example",
        ),
        files.replace("\"SQL timeout\"", "unknown format"),
        files.replace("/example/paste.txt", "not an attachment path"),
        files.replace("\nExplain", "Explain"),
        files.replace("## My request:", "## Other request:"),
        format!("{files}{}", "x".repeat(MAXIMUM_BYTES)),
    ] {
        assert!(project(&text).is_none());
        assert!(matches!(topic_projection(&text), Projection::Keep(_)));
    }
    assert!(project(&format!("{agents}{}", "x".repeat(MAXIMUM_BYTES))).is_none());
}
