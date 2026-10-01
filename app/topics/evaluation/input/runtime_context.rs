//! Opt-in exclusion of complete, narrowly recognized injected skill documents.
//! Canonical content and search embeddings are untouched; unknown prose survives.
use serde::Serialize;
#[cfg(test)]
mod inventory;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::topics::evaluation) enum Policy {
    Preserve,
    OmitSkillDocuments,
}

pub(in crate::topics::evaluation) fn policy() -> Policy {
    if std::env::var_os("MASH_TEST_TOPIC_SKILL_CONTEXT").is_some() {
        Policy::OmitSkillDocuments
    } else {
        Policy::Preserve
    }
}

pub(super) fn omit(text: &str, policy: Policy) -> bool {
    matches!(policy, Policy::OmitSkillDocuments) && skill_document(text)
}

fn field<'a>(text: &'a str, tag: &str) -> Option<(&'a str, &'a str)> {
    let body = text.trim_start().strip_prefix(&format!("<{tag}>"))?;
    let (value, rest) = body.split_once(&format!("</{tag}>"))?;
    if value.is_empty() || value.contains(['<', '>', '\r', '\n']) {
        return None;
    }
    Some((value, rest))
}

fn skill_document(text: &str) -> bool {
    if text.len() > 65_536 {
        return false;
    }
    let Some(body) = text
        .strip_prefix("<skill>")
        .and_then(|s| s.strip_suffix("</skill>"))
    else {
        return false;
    };
    if body.contains("<skill>") || body.contains("</skill>") {
        return false;
    }
    let Some((name, rest)) = field(body, "name") else {
        return false;
    };
    if name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:./".contains(&b))
    {
        return false;
    }
    let Some((path, rest)) = field(rest, "path") else {
        return false;
    };
    if !super::framing::environment::absolute_path(path) || !path.ends_with("/SKILL.md") {
        return false;
    }
    let Some(rest) = rest.trim_start().strip_prefix("---\n") else {
        return false;
    };
    let Some((frontmatter, document)) = rest.split_once("\n---\n") else {
        return false;
    };
    // Recognize only the observed two-field header, not arbitrary YAML/XML.
    let Some(rest) = frontmatter.strip_prefix("name: ") else {
        return false;
    };
    let Some((declared, description)) = rest.split_once("\ndescription: ") else {
        return false;
    };
    declared == name
        && !description.trim().is_empty()
        && !description.contains(['\n', '\r'])
        && !document.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    const DOCUMENT: &str = "<skill>\n<name>example</name>\n<path>/skills/example/SKILL.md</path>\n---\nname: example\ndescription: Inspect database issues.\n---\n\n# Example\nUse the diagnostic tool.\n</skill>";

    #[test]
    fn excluding_context_keeps_replacement_vector_and_source_positions_aligned() {
        let mut thread = crate::topics::evaluation::anchored::thread(&[0, 1]);
        thread.user_sources[0] = DOCUMENT.into();
        thread.user_sources[1] =
            "# AGENTS.md instructions\n<INSTRUCTIONS>Use Git.</INSTRUCTIONS>\nInvestigate SQLite."
                .into();
        let threads = std::collections::BTreeMap::from([("fixture".into(), thread)]);
        let before = super::super::prepare(&threads, Policy::Preserve, None).unwrap();
        let after = super::super::prepare(&threads, Policy::OmitSkillDocuments, None).unwrap();
        assert_eq!(before.texts, after.texts);
        assert_eq!(before.plans[0].projected, [(1, 0)]);
        assert_eq!(after.plans[0].projected, [(0, 0)]);
        assert_eq!(after.plans[0].messages.len(), 1);
        assert_eq!(after.plans[0].sources.len(), 1);
        assert_eq!(after.plans[0].sources[0].text, "Investigate SQLite.");
        assert_eq!(after.plans[0].omitted_skill_documents, 1);
        assert_eq!(threads["fixture"].user_sources[0].text, DOCUMENT);
        assert_eq!(threads["fixture"].vectors.len(), 2);
    }

    #[test]
    fn only_complete_known_injected_documents_are_omitted() {
        assert!(omit(DOCUMENT, Policy::OmitSkillDocuments));
        assert!(!omit(DOCUMENT, Policy::Preserve));
        for unknown in [
            format!("{DOCUMENT}\nWhy does Redis time out?"),
            format!("Explain this:\n{DOCUMENT}"),
            format!("```xml\n{DOCUMENT}\n```"),
            DOCUMENT.replace("</skill>", ""),
            DOCUMENT.replace("<skill>", "<skill kind=\"custom\">"),
            DOCUMENT.replace("name: example", "name: other"),
            DOCUMENT.replace("<path>", "<other>"),
            DOCUMENT.replace("/SKILL.md", "/notes.md"),
            DOCUMENT.replace("/skills/example/SKILL.md", "relative/SKILL.md"),
            DOCUMENT.replace("# Example", "<skill>nested</skill>"),
            DOCUMENT.replace(
                "description: Inspect database issues.",
                "description: |\n  Inspect database issues.",
            ),
            DOCUMENT.replace("# Example", &"x".repeat(65_536)),
        ] {
            assert!(!omit(&unknown, Policy::OmitSkillDocuments));
        }
    }
}
