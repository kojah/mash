use super::*;

#[test]
fn option_acknowledgments_do_not_supply_topic_words() {
    for text in [
        "sure let’s do (1) and (2)",
        "great. let’s do (1)",
        "also sure, let's do (1) and (2) and (3)",
        "sure why don’t we go ahead and do (1) and (2)",
        "alright. let’s do (2), followed by (1) overnight then",
        "maybe we can do (1) then",
        "sounds good let’s do (A)",
        "yes let's do 1 and 2.",
        "(1) yes let’s do it.",
    ] {
        assert_eq!(mask(text), "", "{text}");
    }
}

#[test]
fn subjects_numbers_literals_and_incomplete_choices_are_preserved() {
    for text in [
        "hello world",
        "Go language",
        "let's do PostgreSQL",
        "let's do (Go)",
        "let's do (1) for PostgreSQL",
        "let's do (A) and benchmark Redis",
        "let's do (A)ES",
        "let's do (1) and",
        "let's do (A/B)",
        "let's do -1",
        "let's do 1.2",
        "let's do 1000",
        "let's do (1) + (2)",
        "let's do (1) and (",
        "let's do (1) (",
        "let's do (1) ()",
        "let's do (1))",
        "let's do ((1))",
        "`sounds good` is a parser fixture",
        "say \"hello\" in the test",
        "use 90 days of history",
        "let's do (A) with voyage-code-4",
        "let's do (1) mañana",
        "hello()",
        "/hello",
        "hello.rs",
        "please.yml",
        "file.hello",
        "hello.please",
        "../hello",
        "hi_there",
    ] {
        assert_eq!(mask(text), text, "{text}");
    }
    let long = format!("sounds good let's do (A){}", " ".repeat(300));
    assert_eq!(mask(&long), long);
}

#[test]
fn complete_acknowledgment_sentences_leave_real_subjects_and_boundaries() {
    assert_eq!(
        mask("yes! sounds good.\nAdd tracing policy to AGENTS.md."),
        "..\nAdd tracing policy to AGENTS.md."
    );
    assert_eq!(
        mask("Redis. sounds good. PostgreSQL."),
        "Redis.. PostgreSQL."
    );
    assert_eq!(
        mask("sounds good, investigate SQLite"),
        "sounds good, investigate SQLite"
    );
    assert_eq!(
        mask("Hello world. Go language."),
        "Hello world. Go language."
    );
}
