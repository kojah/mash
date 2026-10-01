//! Hide resolved Markdown destinations, retaining original visible source spans.
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};

pub(super) fn visible(text: &str) -> String {
    // The caller bounds input before parsing. Preserve unknown/malformed syntax
    // and literal code: only parser-confirmed links and image alt text qualify.
    let mut hidden = vec![false; text.len()];
    let mut links = Vec::new();
    let parser = Parser::new(text);
    for (_, definition) in parser.reference_definitions().iter() {
        hidden[definition.span.clone()].fill(true);
    }
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Link { link_type, .. } | Tag::Image { link_type, .. }) => {
                let mask = !matches!(link_type, LinkType::Autolink | LinkType::Email);
                if mask {
                    hidden[range].fill(true);
                }
                links.push(mask);
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                links.pop();
            }
            Event::Text(_) | Event::Code(_) | Event::Html(_) | Event::InlineHtml(_)
                if links.iter().any(|masked| *masked) =>
            {
                hidden[range].fill(false);
            }
            _ => {}
        }
    }
    text.char_indices()
        .map(|(index, character)| if hidden[index] { '.' } else { character })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topics::labels::terms;

    #[test]
    fn visible_domains_survive_without_link_targets_or_titles() {
        let text = "[deadlock recall audit](/home/example/benchmarks/mined-results.md \"hidden description\") and [Redis](https://example.com/irrelevant?q=tracing).";
        let words = terms(&visible(text));
        for word in ["deadlock", "recall", "audit", "redis"] {
            assert!(words.contains_key(word));
        }
        for word in [
            "home",
            "example",
            "benchmarks",
            "mined",
            "results",
            "hidden",
            "description",
            "irrelevant",
            "tracing",
        ] {
            assert!(!words.contains_key(word), "{word}");
        }
    }

    #[test]
    fn nested_formatting_code_labels_and_images_keep_visible_text() {
        let text = "[**SQLite** `pgvector` [recall] details](docs/hidden_(topic).md) ![database schema](hidden/image.png)";
        let words = terms(&visible(text));
        for word in [
            "sqlite", "pgvector", "recall", "details", "database", "schema",
        ] {
            assert!(words.contains_key(word), "{word}");
        }
        for word in ["docs", "hidden", "topic", "image", "png"] {
            assert!(!words.contains_key(word), "{word}");
        }
    }

    #[test]
    fn resolved_references_hide_the_reference_identifier() {
        let text = "[deadlock][hidden-reference]\n\n[hidden-reference]: /hidden/location.md";
        let words = terms(&visible(text));
        assert!(words.contains_key("deadlock"));
        assert!(!words.contains_key("reference"));
        assert!(!words.contains_key("location"));
    }

    #[test]
    fn literal_code_autolinks_and_unknown_syntax_are_preserved() {
        for text in [
            "`[Redis](/tmp/example.md)`",
            "```text\n[Redis](/tmp/example.md)\n```",
            "[unfinished](path",
            "[unknown][reference]",
            "<https://redis.io>",
            "<author@example.com>",
            "[bracket text]",
            "plain text",
        ] {
            assert_eq!(visible(text), text, "{text}");
        }
    }

    #[test]
    fn unicode_and_removed_destinations_do_not_join_adjacent_phrases() {
        let text = "before [数据库](目录/隐藏.md) after [runtime](hidden) confirmation";
        let projected = visible(text);
        assert!(projected.contains("数据库"));
        assert!(!projected.contains("隐藏"));
        assert!(!projected.contains("runtimeconfirmation"));
    }
}
