//! Put factual subjects before repeated worker boilerplate for bounded models.
//! Retain unknown context; this is derived evidence, not instruction execution.
mod feedback;
const MAXIMUM_BYTES: usize = 65_536;
const PR_HEADER: &str = "Revise the supplied GitHub pull request title and description into the author's writing style.";

pub(super) fn project(text: &str) -> Option<String> {
    let text = text.trim();
    if text.len() > MAXIMUM_BYTES {
        return None;
    }
    if text.starts_with(PR_HEADER) {
        let (instructions, source) = text.split_once("\nSource PR description:\n\n")?;
        if !instructions
            .contains("Return a JSON object with title, body, revision_notes, and assessment.")
            || source.trim().is_empty()
            || source.contains("\nSource PR description:\n\n")
        {
            return None;
        }
        return Some(format!(
            "Pull request revision.\nSource PR description:\n\n{source}\n\nRevision instructions:\n{instructions}"
        ));
    }
    let (header, rest) = text.split_once('\n')?;
    if !header.starts_with("You are the dedicated UI feedback worker for the ")
        || !header.ends_with(" repository.")
        || !rest
            .trim_start()
            .starts_with("Address the submitted Agentation feedback batch below.")
    {
        return None;
    }
    let (instructions, feedback) = text.split_once("<submitted_feedback>")?;
    let (feedback, tail) = feedback.split_once("</submitted_feedback>")?;
    if feedback.trim().is_empty()
        || feedback.contains("<submitted_feedback>")
        || tail.contains("<submitted_feedback>")
        || tail.contains("</submitted_feedback>")
    {
        return None;
    }
    if feedback::known_instructions(instructions) {
        Some(format!(
            "UI feedback.\n<submitted_feedback>{feedback}</submitted_feedback>\n{}",
            feedback::tail(tail)
        ))
    } else {
        Some(format!(
            "UI feedback.\n<submitted_feedback>{feedback}</submitted_feedback>\n\nWorker instructions:\n{instructions}{tail}"
        ))
    }
}

#[test]
fn factual_payload_precedes_worker_instructions_without_losing_trailing_requests() {
    let instructions = format!(
        "{PR_HEADER}\nReturn a JSON object with title, body, revision_notes, and assessment."
    );
    let source = "Close the gzip writer on export errors.\nWriting skill: preserve meaning.\nAlso investigate PostgreSQL timeouts.";
    let text = format!("{instructions}\nSource PR description:\n\n{source}");
    let projected = project(&text).unwrap();
    assert!(
        projected
            .starts_with("Pull request revision.\nSource PR description:\n\nClose the gzip writer")
    );
    assert!(projected.contains(&instructions));
    assert!(projected.contains(source));
    let header = "You are the dedicated UI feedback worker for the example repository.\n\nAddress the submitted Agentation feedback batch below. Preserve unrelated work.\n";
    let feedback = "Fix horizontal overflow in the sidebar.";
    let tail = "\nSave the result.\nAlso fix syntax highlighting.";
    let projected = project(&format!(
        "{header}<submitted_feedback>{feedback}</submitted_feedback>{tail}"
    ))
    .unwrap();
    assert!(projected.starts_with("UI feedback.\n<submitted_feedback>Fix horizontal overflow"));
    assert!(projected.contains(header));
    assert!(projected.contains(tail));
}

#[test]
fn unknown_quoted_nested_incomplete_and_oversized_templates_are_preserved() {
    let ui = "You are the dedicated UI feedback worker for the example repository.\nAddress the submitted Agentation feedback batch below.\n<submitted_feedback>Fix CSS.</submitted_feedback>";
    let pr = format!(
        "{PR_HEADER}\nReturn a JSON object with title, body, revision_notes, and assessment.\nSource PR description:\n\nClose gzip resources."
    );
    for text in [
        format!("```\n{ui}\n```"),
        ui.replace("</submitted_feedback>", ""),
        ui.replace(
            "Fix CSS.",
            "<submitted_feedback>Fix CSS.</submitted_feedback>",
        ),
        ui.replace("Fix CSS.", ""),
        format!("{ui}<submitted_feedback>second</submitted_feedback>"),
        ui.replace(
            "Address the submitted Agentation feedback batch below.",
            "Explain the submitted feedback tag.",
        ),
        pr.replace(
            "Return a JSON object with title, body, revision_notes, and assessment.",
            "Unknown instructions.",
        ),
        format!("{pr}\nSource PR description:\n\nsecond"),
        format!("{pr}{}", "x".repeat(MAXIMUM_BYTES)),
    ] {
        assert!(project(&text).is_none());
    }
}
