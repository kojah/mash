use super::*;
use std::fmt::Write;

const PAGE: &str = "## Page Feedback: /analyzers/channelcapacity/\n**Viewport:** 1268×653\n\n### 1. h3 \"Checks\"\n**Location:** .content-panel > table > tr:nth-of-type(2) > td\n**Feedback:** The table is broken. Fix the missing detections.\n";

#[test]
fn requests_precede_retained_page_and_element_context_without_coordinates() {
    let projected = project(PAGE).unwrap();
    assert_eq!(
        projected,
        "The table is broken. Fix the missing detections.\nh3 \"Checks\"\n\nPage: /analyzers/channelcapacity/"
    );
    let multiple = format!(
        "{PAGE}\n### 2. button \"Contributing\"\n**Location:** .sidebar > #contributing\n**Feedback:** Move Development under Contributing.\nPreserve the installation link."
    );
    let projected = project(&multiple).unwrap();
    assert!(projected.contains("Move Development under Contributing.\nPreserve the installation link.\nbutton \"Contributing\""));
    assert_eq!(projected.matches("Page:").count(), 1);
}

#[test]
fn unknown_metadata_and_ambiguous_or_incomplete_annotations_keep_original_payload() {
    for text in [
        PAGE.replace("1268×653", "narrow viewport"),
        PAGE.replace("1268×653", "0×653"),
        PAGE.replace("1268×653", "999999999999999×653"),
        PAGE.replace("### 1.", "### 2."),
        PAGE.replace("h3 \"Checks\"", ""),
        PAGE.replace("**Location:**", "**Nearby text:**"),
        PAGE.replace(
            ".content-panel > table > tr:nth-of-type(2) > td",
            "Inspect the database timeout first",
        ),
        PAGE.replace("**Feedback:** ", "**Feedback:**\n"),
        PAGE.replace("The table is broken. Fix the missing detections.", ""),
        format!("{PAGE}\n**Screenshot details:** Preserve these observations."),
        format!("{PAGE}\n### 2. incomplete"),
        format!("```\n{PAGE}\n```"),
        format!("{PAGE}{}", "x".repeat(MAXIMUM_BYTES)),
    ] {
        assert!(project(&text).is_none(), "{text}");
    }
}

#[test]
fn oversized_annotation_batches_preserve_every_request_instead_of_truncating() {
    let mut text = "## Page Feedback: /example/\n**Viewport:** 1000×800\n".to_owned();
    for number in 1..=33 {
        write!(
            text,
            "\n### {number}. button\n**Location:** .button\n**Feedback:** Keep request {number}.\n"
        )
        .unwrap();
    }
    assert!(project(&text).is_none());
}
