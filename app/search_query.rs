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

/// Explicit references and quoted phrases are constraints, not approximate topics.
pub(crate) fn matches_reference(content: &str, query: &str) -> bool {
    let requested = references(query);
    let actual = references(content);
    requested.iter().all(|number| actual.contains(number))
        && matches_phrases(content, query)
        && {
            let scoped = scoped_references(query);
            scoped.is_empty() || {
                let available = scoped_references(content);
                scoped.iter().all(|reference| available.contains(reference))
            }
        }
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

fn matches_phrases(content: &str, query: &str) -> bool {
    let phrases = query
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, phrase)| terms(phrase).collect::<Vec<_>>())
        .filter(|phrase| !phrase.is_empty())
        .collect::<Vec<_>>();
    if phrases.is_empty() {
        return true;
    }
    let words = terms(content).collect::<Vec<_>>();
    phrases.iter().all(|phrase| {
        words.windows(phrase.len()).any(|window| {
            window
                .iter()
                .zip(phrase)
                .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
        })
    })
}

#[derive(Debug, PartialEq, Eq)]
enum ReferenceKind {
    Pull,
    Issue,
}

#[derive(Debug, PartialEq, Eq)]
struct Reference<'a> {
    kind: ReferenceKind,
    number: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
struct ScopedReference {
    host: String,
    port: Option<u16>,
    repository: String,
    kind: ReferenceKind,
    number: String,
}

// Parse links locally; never fetch them. Number-only queries retain their broad
// scope, while a complete reference URL also identifies a host/repository/kind.
fn scoped_references(text: &str) -> Vec<ScopedReference> {
    text.split(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']')
    })
    .filter_map(|token| {
        if !token.contains("://") {
            return None;
        }
        let url = reqwest::Url::parse(token.trim_end_matches([',', '.', ';', '!', '?'])).ok()?;
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        let segments = url.path_segments()?.collect::<Vec<_>>();
        let (index, kind) = segments.iter().enumerate().find_map(|(index, segment)| {
            let number = *segments.get(index + 1)?;
            if index == 0 || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            match *segment {
                "pull" => Some((index, ReferenceKind::Pull)),
                "issues" | "issue" => Some((index, ReferenceKind::Issue)),
                _ => None,
            }
        })?;
        let number = *segments.get(index + 1)?;
        let host = url.host_str()?.to_owned();
        let repository = segments[..index].join("/");
        Some(ScopedReference {
            repository: if host == "github.com" {
                repository.to_ascii_lowercase()
            } else {
                repository
            },
            host,
            port: url.port(),
            kind,
            number: number.to_owned(),
        })
    })
    .collect()
}

pub(crate) fn has_reference(query: &str) -> bool {
    !references(query).is_empty()
}

fn references(text: &str) -> Vec<Reference<'_>> {
    let words = terms(text).collect::<Vec<_>>();
    words
        .windows(2)
        .filter_map(|pair| {
            let kind = if ["pr", "pull"]
                .iter()
                .any(|word| pair[0].eq_ignore_ascii_case(word))
            {
                ReferenceKind::Pull
            } else if ["issue", "issues"]
                .iter()
                .any(|word| pair[0].eq_ignore_ascii_case(word))
            {
                ReferenceKind::Issue
            } else {
                return None;
            };
            Some(Reference {
                kind,
                number: pair[1],
            })
        })
        .filter(|reference| reference.number.bytes().all(|byte| byte.is_ascii_digit()))
        .chain(words.windows(3).filter_map(|triple| {
            (triple[0].eq_ignore_ascii_case("pull")
                && triple[1].eq_ignore_ascii_case("request")
                && triple[2].bytes().all(|byte| byte.is_ascii_digit()))
            .then_some(Reference {
                kind: ReferenceKind::Pull,
                number: triple[2],
            })
        }))
        .chain(words.iter().filter_map(|word| {
            word.get(..2)
                .filter(|prefix| prefix.eq_ignore_ascii_case("pr"))
                .and_then(|_| word.get(2..))
                .filter(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                })
                .map(|number| Reference {
                    kind: ReferenceKind::Pull,
                    number,
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
        assert!(matches_reference("review Pr180", "pull request #180"));
        assert!(!matches_reference("review issue 180", "PR 180"));
        assert!(!matches_reference("review PR 180", "issue 180"));
    }

    #[test]
    fn full_reference_urls_do_not_match_another_repository_or_reference_kind() {
        let query = "https://github.com/org/mash/pull/180";
        assert!(matches_reference(
            "review https://github.com/org/mash/pull/180",
            query
        ));
        for content in [
            "review https://github.com/org/other/pull/180",
            "review https://github.com/org/mash/issues/180",
            "review https://git.example/org/mash/pull/180",
            "review PR 180",
        ] {
            assert!(!matches_reference(content, query), "{content}");
        }
        assert!(matches_reference(
            "See [the fix](https://GITHUB.com/Org/Mash/pull/180/files#diff) please.",
            query
        ));
        assert!(matches_reference(
            "review https://github.com/org/other/pull/180",
            "PR 180"
        ));
        assert!(!matches_reference(
            "https://git.example:8443/org/mash/pull/180",
            "https://git.example/org/mash/pull/180"
        ));
    }

    #[test]
    fn quoted_phrases_require_contiguous_literal_words() {
        assert!(matches_reference(
            "Maintain one JOINT-index for the group",
            "\"joint index\""
        ));
        assert!(!matches_reference(
            "joint pain and body mass index",
            "\"joint index\""
        ));
        assert!(!matches_reference(
            "one joint index",
            "\"joint index\" \"node group\""
        ));
        assert!(matches_reference(
            "one joint index per node group",
            "\"joint index\" \"node group\""
        ));
    }

    #[test]
    fn reference_routes_are_distinguished_from_repository_names() {
        for repository in ["pull", "issues"] {
            let query = format!("https://github.com/org/{repository}/pull/180");
            assert!(matches_reference(&query, &query));
            assert!(!matches_reference(
                "https://github.com/org/other/pull/180",
                &query
            ));
        }
    }
}
