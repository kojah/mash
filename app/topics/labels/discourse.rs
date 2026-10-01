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
        "let s tackle those next items then",
        "let s go ahead and implement then",
        "let s do all of em then",
        "let s do that instead then",
        "let s optimize it then",
        "let s fix all of these",
        "let s fix all of em",
        "let s keep going then",
        "let s continue then",
        "let s do that then",
        "let s do it then",
        "actually nvm just fix em",
        "nvm just fix em",
        "can we keep investigating",
        "keep investigating",
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
        "very nice",
        "nice",
        "i agree",
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
        "sounds good let s do it then",
        "sure let s tackle those next items then",
        "sounds good let s do all of em then",
        "actually nvm just fix em",
        "nice nice nice let s do it",
        "sounds good let s optimize it then",
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
        "let s fix all of these redis indexes",
        "nvm just fix em by batching sqlite writes",
        "keep investigating the race issue",
        "nice tracing policy",
        "let s do it then measure latency",
        "yes let s do a and b",
        "nvm",
    ] {
        assert!(!only(text), "{text}");
    }
}
