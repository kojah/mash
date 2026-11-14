use super::*;

#[derive(serde::Deserialize)]
struct Fixture {
    name: String,
    text: String,
    secrets: Vec<String>,
}

#[test]
fn synthetic_credentials_and_benign_chat_contract() {
    let mut fixtures: Vec<Fixture> =
        serde_json::from_str(include_str!("fixtures/fixtures.json")).unwrap();
    fixtures.extend(
        serde_json::from_str::<Vec<Fixture>>(include_str!("fixtures/additional.json")).unwrap(),
    );
    fixtures.extend(
        serde_json::from_str::<Vec<Fixture>>(include_str!("fixtures/audit-regressions.json"))
            .unwrap(),
    );
    for fixture in fixtures {
        let safe = text(&fixture.text);
        assert_eq!(text(&safe), safe, "idempotence: {}", fixture.name);
        for secret in &fixture.secrets {
            assert!(
                !safe.contains(secret),
                "missed credential: {}",
                fixture.name
            );
            let masked = projection(&fixture.text);
            for (start, value) in fixture.text.match_indices(secret.as_str()) {
                assert!(
                    masked[start..start + value.len()]
                        .chars()
                        .all(|c| matches!(c, '*' | '×' | '█' | '🟥')),
                    "partially masked credential: {}",
                    fixture.name
                );
            }
        }
        if fixture.secrets.is_empty() {
            assert_eq!(
                safe, fixture.text,
                "changed benign content: {}",
                fixture.name
            );
        }
    }
}

#[test]
fn nested_serialization_masks_values_without_eating_following_content() {
    for value in [
        r#"password="pa\"ssword" tail-kept"#,
        r#"password="pa\\" tail-kept"#,
    ] {
        let mut source = value.to_owned();
        let mut expected = "password=\"[REDACTED_SECRET]\" tail-kept".to_owned();
        for _ in 0..4 {
            assert_eq!(text(&source), expected);
            let masked = projection(&source);
            assert_eq!(masked.len(), source.len());
            assert_eq!(masked.chars().count(), source.chars().count());
            assert!(masked.contains("tail-kept"));
            source = serde_json::to_string(&source).unwrap();
            expected = serde_json::to_string(&expected).unwrap();
        }
    }
    assert_eq!(
        text(r#"password=\"unfinished"#),
        r#"password=\"[REDACTED_SECRET]"#
    );
}

#[test]
fn offset_masking_and_fail_closed_limits() {
    let source = "🔑 password=\"café-東京-🗝\" next";
    let safe = projection(source);
    assert_eq!(source.len(), safe.len());
    assert_eq!(source.chars().count(), safe.chars().count());
    assert!(!safe.contains("café"));
    let large = "x".repeat(2 * 1024 * 1024 + 1);
    assert_eq!(text(&large), "[REDACTED_SECRET]");
    let spam = "password=\"hunter2\" ".repeat(4100);
    assert_eq!(text(&spam), "[REDACTED_SECRET]");
    assert_eq!(
        text("-----BEGIN PRIVATE KEY-----\nunfinished body"),
        "[REDACTED_SECRET]"
    );
}

#[test]
fn private_spans_are_masked_only_when_closed() {
    let source = "keep <private>home address</private> and <PRIVATE>\nphone\n</Private> too";
    let masked = private(source);
    assert_eq!(
        masked,
        "keep <private>[PRIVATE]</private> and <PRIVATE>[PRIVATE]</Private> too"
    );
    assert_eq!(private(&masked), masked);
    for benign in [
        "wrap it in <private> tags",
        "an empty <private></private> pair",
    ] {
        assert!(matches!(private(benign), std::borrow::Cow::Borrowed(_)));
    }
    // Output masking is for credentials; private spans are gone before storage.
    assert_eq!(text("<private>x</private>"), "<private>x</private>");
}

#[test]
fn a_disabled_policy_passes_text_through() {
    let policy = super::Policy::new(false);
    assert_eq!(policy.text("password=\"hunter2\""), "password=\"hunter2\"");
}
