//! Recognize complete conversational continuations, never partial requests.
// The caller normalizes punctuation/case and bounds input to 256 characters.
// Every token must be consumed by a known phrase. Unknown words (including
// technical subjects and option identifiers) preserve the original evidence.
pub(super) fn only(normalized: &str) -> bool {
    const PHRASES: &[&str] = &[
        "request interrupted by user for tool use",
        "request interrupted by user",
        "request interrupted",
        "continue from where you left off",
        "continue where you left off",
        "pick up where you left off",
        "let s keep going",
        "let s go ahead",
        "let s continue",
        "let s do that",
        "let s do it",
        "keep on going",
        "keep going",
        "go ahead",
        "go on",
        "sounds good",
        "thank you",
        "try again",
        "hello",
        "hi",
        "hey",
        "thanks",
        "yes",
        "yeah",
        "no",
        "ok",
        "okay",
        "alright",
        "sure",
        "fine",
        "great",
        "continue",
        "please",
    ];
    let mut rest = normalized;
    while !rest.is_empty() {
        let Some(tail) = PHRASES.iter().find_map(|phrase| {
            rest.strip_prefix(phrase)
                .filter(|tail| tail.is_empty() || tail.starts_with(' '))
        }) else {
            return false;
        };
        rest = tail.trim_start();
    }
    true
}

#[test]
fn complete_continuations_are_not_new_topic_evidence() {
    for text in [
        "alright sounds good let s keep going",
        "yeah sure go ahead",
        "continue from where you left off",
        "please try again",
        "request interrupted by user for tool use",
    ] {
        assert!(only(text), "{text}");
    }
    for text in [
        "sounds good let s do postgres",
        "continue with tracing",
        "go language",
        "yes let s do a",
        "thanks but fix the deadlock",
        "hello world",
        "surely",
        "request interrupted by user for tool use explain why",
    ] {
        assert!(!only(text), "{text}");
    }
}
