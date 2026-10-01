use super::*;
use crate::topics::evaluation::input::{prepare, runtime_context};

fn stored(text: &str, message: &str) -> Source {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.query_row(
        "SELECT ?1 AS canonical_content, ?2 AS message_id, 0 AS ordinal, 0 AS stored_projection",
        rusqlite::params![text, message],
        |row| Source::stored(row, text.to_owned()),
    ).unwrap()
}

fn manifest(text: &str) -> String {
    format!(
        "schema_version = 1\n[[sources]]\nthread = 'fixture'\nprovider = 'codex'\nmessage_id = 'startup'\ncanonical_blake3 = '{}'\ncategory = 'generated_startup'\nreview = 'Full source reviewed as generated context.'\n",
        blake3::hash(text.as_bytes())
    )
}

fn thread(text: &str) -> Thread {
    let mut thread = crate::topics::evaluation::anchored::thread(&[0, 1]);
    thread.user_sources = vec![
        stored(text, "startup"),
        stored(
            "# AGENTS.md instructions\n<INSTRUCTIONS>Use Git.</INSTRUCTIONS>\nInvestigate SQLite.",
            "request",
        ),
    ];
    thread
}

#[test]
fn reviewed_omission_preserves_requests_and_replacement_positions() {
    let text = "[gas-city] example • 2026-08-24T17:30:58\nGenerated startup context.";
    let selection = Selection::parse(&manifest(text)).unwrap();
    let threads = BTreeMap::from([("fixture".into(), thread(text))]);
    let before = prepare(&threads, runtime_context::Policy::Preserve, None).unwrap();
    let after = prepare(
        &threads,
        runtime_context::Policy::Preserve,
        Some(&selection),
    )
    .unwrap();
    assert_eq!(before.texts, after.texts);
    assert_eq!(before.plans[0].projected, [(1, 0)]);
    assert_eq!(after.plans[0].projected, [(0, 0)]);
    assert_eq!(after.plans[0].omitted_startup_context, 1);
    assert_eq!(after.plans[0].omitted_skill_documents, 0);
    assert_eq!(after.plans[0].sources[0].text, "Investigate SQLite.");
    assert_eq!(threads["fixture"].user_sources[0].text, text);
    assert_eq!(threads["fixture"].vectors.len(), 2);
    assert!(!selection.omits("other", &stored(text, "startup")));
    assert!(!selection.omits("fixture", &stored(text, "other")));
    assert!(!selection.omits(
        "fixture",
        &stored(&format!("{text}\nExplain the workflow."), "startup")
    ));
    assert!(!selection.omits("fixture", &Source::from(text)));
    assert!(!selection.omits(
        "fixture",
        &stored(text, "startup").projected("workflow".into())
    ));
}

#[test]
fn stale_missing_projected_or_wrong_provider_sources_fail_before_projection() {
    let text = "Generated startup context.";
    let selection = Selection::parse(&manifest(text)).unwrap();
    let mut altered = thread(&format!("{text}\nPlease discuss this workflow."));
    assert!(
        selection
            .validate(&BTreeMap::from([("fixture".into(), altered.clone())]))
            .is_err()
    );
    altered = thread(text);
    altered.provider = Provider::ClaudeCode;
    assert!(
        selection
            .validate(&BTreeMap::from([("fixture".into(), altered.clone())]))
            .is_err()
    );
    altered = thread(text);
    altered.user_sources[0] = altered.user_sources[0].projected(text.into());
    assert!(
        selection
            .validate(&BTreeMap::from([("fixture".into(), altered)]))
            .is_err()
    );
    assert!(selection.validate(&BTreeMap::new()).is_err());
    let mut missing = thread(text);
    missing.user_sources[0] = stored(text, "different");
    assert!(
        prepare(
            &BTreeMap::from([("fixture".into(), missing)]),
            runtime_context::Policy::Preserve,
            Some(&selection)
        )
        .is_err()
    );
}

#[test]
fn reviewed_manifest_is_strict_bounded_and_rejects_ambiguous_entries() {
    let valid = manifest("Generated startup context.");
    for invalid in [
        valid.replace("schema_version = 1", "schema_version = 2"),
        valid.replace("category = 'generated_startup'", "category = 'human'"),
        valid.replace("provider = 'codex'", "provider = 'unknown'"),
        valid.replace("thread = 'fixture'", "thread = ''"),
        valid.replace(
            "review = 'Full source reviewed as generated context.'",
            "review = ''",
        ),
        valid.replace("message_id = 'startup'", "message_id = ''"),
        valid.replace(
            &blake3::hash(b"Generated startup context.").to_string(),
            "invalid",
        ),
        format!("{valid}unknown = true\n"),
        format!(
            "{valid}[[sources]]{}",
            valid.split_once("[[sources]]").unwrap().1
        ),
        "schema_version = 1\nsources = []".into(),
        "x".repeat(MAXIMUM_BYTES + 1),
    ] {
        assert!(Selection::parse(&invalid).is_err());
    }
    let selection = Selection::parse(&valid).unwrap();
    assert!(matches!(
        Selection::policy(Some(&selection)),
        Policy::ReviewedStartup {
            reviewed_sources: 1,
            ..
        }
    ));
    assert!(matches!(Selection::policy(None), Policy::Preserve));
}
