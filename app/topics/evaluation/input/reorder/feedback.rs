//! Exact known UI-worker routing wrappers; unknown framing stays evidence.
const START: &str = "Address the submitted Agentation feedback batch below. Work only inside this repository. Preserve unrelated working-tree changes. ";
const CHECKS: &str = "Inspect the current source rather than assuming a framework or file location. Make the requested changes, run focused checks appropriate to the files you touched, and verify the result as far as the local environment allows.";
const STATUS: &str = "Return status \"completed\" only when every annotation in this batch has been addressed and the relevant checks pass. Return status \"blocked\" when clarification or unavailable prerequisites prevent completion, and explain the blocker in summary.";
const OLD_RULES: &str = "Do not commit or push. Do not modify the Agentation review tooling unless the feedback explicitly concerns that tooling. ";
const RULES: &str = "Do not commit or push. Do not modify the Agentation review integration unless the feedback explicitly concerns it.";
const SAVE: &str = "When you have finished, write your result as your final action to the file ";
const RESULT: &str = ": a single JSON object with exactly two fields, \"status\" (either \"completed\" or \"blocked\") and \"summary\" (a short explanation). Write only that JSON object to that file.";

pub(super) fn known_instructions(instructions: &str) -> bool {
    let Some((_, body)) = instructions.split_once('\n') else {
        return false;
    };
    let Some(body) = body.trim().strip_prefix(START) else {
        return false;
    };
    body == format!("{OLD_RULES}{CHECKS}\n\n{STATUS}")
        || body == format!("{CHECKS}\n\n{RULES}\n\n{STATUS}")
}

/// Remove only the known save-result sentence; preserve appended user requests.
#[allow(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "recognize the exact lowercase suffix of the known routing protocol, not arbitrary files"
)]
pub(super) fn tail(text: &str) -> &str {
    let Some(rest) = text.trim_start().strip_prefix(SAVE) else {
        return text;
    };
    let Some((path, remaining)) = rest.split_once(RESULT) else {
        return text;
    };
    if path.starts_with('/') && path.ends_with(".json") && !path.contains(['\n', '\r', '<', '>']) {
        remaining
    } else {
        text
    }
}

#[test]
fn known_worker_wrappers_leave_payload_and_unrecognized_requests() {
    let header = "You are the dedicated UI feedback worker for the example repository.";
    let instructions = format!("{header}\n\n{START}{OLD_RULES}{CHECKS}\n\n{STATUS}\n\n");
    assert!(known_instructions(&instructions));
    assert!(known_instructions(&format!(
        "{header}\n\n{START}{CHECKS}\n\n{RULES}\n\n{STATUS}"
    )));
    assert!(!known_instructions(&format!(
        "{instructions}Also fix layout first."
    )));
    let routing = format!("{SAVE}/example/result.json{RESULT}");
    assert_eq!(tail(&routing), "");
    assert_eq!(
        tail(&format!("{routing}\nAlso fix syntax highlighting.")),
        "\nAlso fix syntax highlighting."
    );
    assert_eq!(
        tail(&routing.replace(".json", ".txt")),
        routing.replace(".json", ".txt")
    );
    assert_eq!(
        tail(&routing.replace(".json", ".JSON")),
        routing.replace(".json", ".JSON")
    );
    assert_eq!(
        tail("Save the result, and investigate timeouts."),
        "Save the result, and investigate timeouts."
    );
    let payload = "## Sidebar\nFix horizontal overflow.";
    let projected = super::project(&format!(
        "{instructions}<submitted_feedback>{payload}</submitted_feedback>\n{routing}"
    ))
    .unwrap();
    assert!(projected.contains(payload));
    assert!(!projected.contains("Worker instructions"));
    assert!(!projected.contains("result.json"));
    let page = "## Page Feedback: /checks/\n**Viewport:** 1200×800\n\n### 1. h3 \"Checks\"\n**Location:** .content-panel > #checks\n**Feedback:** Repair the broken table.";
    let projected = super::project(&format!(
        "{instructions}<submitted_feedback>{page}</submitted_feedback>\n{routing}\nAlso preserve the links."
    ))
    .unwrap();
    assert!(projected.starts_with("Repair the broken table."));
    assert!(projected.contains("h3 \"Checks\""));
    assert!(projected.contains("Page: /checks/"));
    assert!(projected.contains("Also preserve the links."));
    assert!(!projected.contains("Viewport"));
    assert!(!projected.contains("content-panel"));
    let unknown = page.replace("**Location:**", "**Nearby text:**");
    let projected = super::project(&format!(
        "{instructions}<submitted_feedback>{unknown}</submitted_feedback>"
    ))
    .unwrap();
    assert!(projected.contains(&unknown));
}
