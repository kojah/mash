//! Literal SQLite query compilation and exact reference matching.

pub(crate) fn has_terms(query: &str) -> bool {
    query.chars().any(char::is_alphanumeric)
}

pub(crate) fn validate(query: &str) -> Result<(), crate::AppError> {
    if !has_terms(query) || query.len() > 16384 {
        return Err(crate::AppError::usage(
            "search query must contain letters or numbers and be at most 16384 bytes",
        ));
    }
    if query.chars().any(|character| character.is_alphabetic() && !matches!(u32::from(character), 0..=0x024f | 0x1e00..=0x1eff | 0x2c60..=0x2c7f | 0xa720..=0xa7ff | 0xab30..=0xab6f)) {
        return Err(crate::AppError::usage("the current search models support English queries in Latin script; queries in other scripts are unsupported"));
    }
    if !query.contains('"') && terms(query).all(is_stop_word) {
        return Err(crate::AppError::usage(
            "enter a more specific search query; common words alone do not identify a topic",
        ));
    }
    Ok(())
}

pub(crate) fn is_stop_word(word: &str) -> bool {
    const WORDS: &[&str] = &[
        "a", "an", "about", "and", "are", "as", "at", "by", "can", "could", "did", "does", "for",
        "from", "how", "in", "into", "is", "it", "of", "on", "our", "that", "the", "their",
        "there", "these", "this", "to", "was", "were", "what", "when", "where", "which", "with",
        "would", "you", "your",
    ];
    WORDS
        .iter()
        .any(|candidate| word.eq_ignore_ascii_case(candidate))
}

/// Quote every unquoted term and preserve explicitly quoted phrases. Splitting
/// punctuation separates literal words and prevents URL, hyphen,
/// question mark, and reserved-operator input from becoming FTS syntax.
pub(crate) fn sqlite(query: &str) -> String {
    let mut clauses = Vec::new();
    for (index, segment) in query.split('"').enumerate() {
        let words = terms(segment).collect::<Vec<_>>();
        if index % 2 == 1 {
            if !words.is_empty() {
                clauses.push(format!("\"{}\"", words.join(" ")));
            }
        } else {
            clauses.extend(words.into_iter().map(|word| format!("\"{word}\"")));
        }
    }
    clauses.join(" AND ")
}

fn terms(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
}

/// Explicit PR/issue numbers are identifiers, not approximate semantic topics.
pub(crate) fn matches_reference(content: &str, query: &str) -> bool {
    let requested = references(query);
    let actual = references(content);
    requested.iter().all(|number| actual.contains(number))
        && terms(query)
            .filter(|term| {
                term.len() >= 3
                    && term.is_ascii()
                    && term.bytes().any(|byte| byte.is_ascii_digit())
                    && term.bytes().any(|byte| byte.is_ascii_alphabetic())
            })
            .filter(|term| {
                !term
                    .get(..2)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("pr"))
            })
            .all(|identifier| terms(content).any(|term| term.eq_ignore_ascii_case(identifier)))
}

pub(crate) fn has_reference(query: &str) -> bool {
    !references(query).is_empty()
}

fn references(text: &str) -> Vec<&str> {
    let words = terms(text).collect::<Vec<_>>();
    words
        .windows(2)
        .filter(|pair| {
            ["pr", "pull", "issue", "issues"]
                .iter()
                .any(|word| pair[0].eq_ignore_ascii_case(word))
        })
        .map(|pair| pair[1])
        .filter(|number| number.bytes().all(|byte| byte.is_ascii_digit()))
        .chain(words.iter().filter_map(|word| {
            word.strip_prefix("PR")
                .or_else(|| word.strip_prefix("pr"))
                .filter(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                })
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_scripts_and_empty_topics_fail_without_rejecting_latin_identifiers() {
        for query in ["日本語の検索", "поиск", "搜索", "!!!", "the and of"] {
            assert!(validate(query).is_err(), "{query}");
        }
        for query in [
            "cBioPortal",
            "voyage-code-4",
            "café caching",
            "PR 180",
            "123",
        ] {
            assert!(validate(query).is_ok(), "{query}");
        }
        assert!(!matches_reference(
            "ordinary discussion",
            "qzxv981723zznohistory"
        ));
        assert!(matches_reference(
            "configure voyage-code-4",
            "voyage-code-4"
        ));
    }

    #[test]
    fn literal_terms_cannot_turn_into_database_syntax() {
        for (query, expected) in [
            ("voyage-code-4", "\"voyage\" AND \"code\" AND \"4\""),
            ("why Redis?", "\"why\" AND \"Redis\""),
            (
                "https://github.com/a/b/pull/180",
                "\"https\" AND \"github\" AND \"com\" AND \"a\" AND \"b\" AND \"pull\" AND \"180\"",
            ),
            ("\"shared index\" nodes", "\"shared index\" AND \"nodes\""),
            (
                "AND OR NOT NEAR *",
                "\"AND\" AND \"OR\" AND \"NOT\" AND \"NEAR\"",
            ),
            ("日本語の検索", "\"日本語の検索\""),
            ("!!!", ""),
        ] {
            assert_eq!(sqlite(query), expected);
        }
        assert!(!has_terms("!!!"));
        assert!(has_terms("日本語"));
    }

    #[test]
    fn explicit_reference_numbers_are_exact() {
        assert!(matches_reference(
            "https://github.com/a/b/pull/180",
            "PR 180"
        ));
        assert!(!matches_reference("review PR 183", "PR 180"));
        assert!(!matches_reference("review PR 1800", "PR #180?"));
        assert!(!matches_reference("indent magnitude 180", "PR 180"));
        assert!(matches_reference("review PR180", "PR 180"));
        assert!(!matches_reference("review PR183", "PR180"));
        assert!(matches_reference("three machines", "3 nodes"));
    }
}
