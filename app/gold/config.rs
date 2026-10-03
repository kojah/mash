//! Active build recipe and independent query policy.
use crate::{AppError, search_results::SearchUnit};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Configuration {
    pub(crate) build: Build,
    pub(crate) query: Query,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Build {
    pub(crate) embedding_text: EmbeddingText,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EmbeddingText {
    #[default]
    Dialogue,
    TitleAndDialogue,
}

impl Build {
    pub(crate) fn generation(&self, model: &str) -> String {
        if self == &Self::default() {
            model.to_owned()
        } else {
            format!("gold-title-v1:{model}")
        }
    }

    pub(crate) fn prepare(&self, dialogue: String, title: Option<&str>) -> String {
        match (
            self.embedding_text,
            title.map(str::trim).filter(|t| !t.is_empty()),
        ) {
            (EmbeddingText::TitleAndDialogue, Some(title)) if !dialogue.is_empty() => {
                // Keep metadata bounded so it cannot crowd out dialogue entirely.
                let title: String = title.chars().take(256).collect();
                format!("Thread: {title}\n\n{dialogue}")
            }
            _ => dialogue,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Query {
    pub(crate) intent: super::exact::Intent,
    pub(crate) unit: SearchUnit,
    pub(crate) limit: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            intent: super::exact::Intent::default(),
            unit: SearchUnit::Threads,
            limit: 20,
        }
    }
}

impl Configuration {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if !(1..=200).contains(&self.query.limit) {
            return Err(AppError::configuration(
                "gold query limit must be between 1 and 200",
            ));
        }
        Ok(())
    }
}

impl Query {
    pub(crate) fn resolve(
        &self,
        limit: Option<usize>,
        unit: Option<SearchUnit>,
    ) -> (usize, SearchUnit) {
        (limit.unwrap_or(self.limit), unit.unwrap_or(self.unit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_are_strict_and_preparation_preserves_visibility_and_unicode() {
        let default: Configuration = toml::from_str("").unwrap();
        assert_eq!(default.build, Build::default());
        assert_eq!(
            default.build.prepare("dialogue".into(), Some("title")),
            "dialogue"
        );
        let config: Configuration =
            toml::from_str("[build]\nembedding_text='title-and-dialogue'").unwrap();
        let build = config.build;
        assert_eq!(
            build.prepare("body".into(), Some(" title ")),
            "Thread: title\n\nbody"
        );
        for title in [None, Some("  ")] {
            assert_eq!(build.prepare("body".into(), title), "body");
        }
        assert_eq!(build.prepare(String::new(), Some("secret")), "");
        let title = "é".repeat(300);
        assert_eq!(
            build.prepare("body".into(), Some(&title)),
            format!("Thread: {}\n\nbody", "é".repeat(256))
        );
        for input in ["[build]\nembedding_text='invalid'", "[build]\nunknown=true"] {
            assert!(toml::from_str::<Configuration>(input).is_err());
        }
    }
}
