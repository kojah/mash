//! Active build recipe and independent query policy.
use crate::{AppError, search_results::SearchUnit};
use rmcp::schemars;
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
    #[serde(skip_serializing_if = "Representations::is_default")]
    pub(crate) representations: Representations,
    pub(crate) embedding_text: EmbeddingText,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_embedding_chars: Option<EmbeddingTextBudget>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Representations {
    #[default]
    LexicalAndDense,
    LexicalOnly,
}

impl Representations {
    // serde's skip_serializing_if hook receives a reference to the field.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_default(&self) -> bool {
        *self == Self::LexicalAndDense
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct EmbeddingTextBudget(u32);

impl TryFrom<u32> for EmbeddingTextBudget {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if (256..=65_536).contains(&value) {
            Ok(Self(value))
        } else {
            Err("max_embedding_chars must be between 256 and 65536")
        }
    }
}

impl From<EmbeddingTextBudget> for u32 {
    fn from(value: EmbeddingTextBudget) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EmbeddingText {
    #[default]
    Dialogue,
    TitleAndDialogue,
}

impl Build {
    pub(crate) fn dense_enabled(&self) -> bool {
        self.representations == Representations::LexicalAndDense
    }

    pub(crate) fn expected_embeddings(&self, searchable_messages: u64) -> u64 {
        if self.dense_enabled() {
            searchable_messages
        } else {
            0
        }
    }

    pub(crate) fn require_intent(&self, intent: super::exact::Intent) -> Result<(), AppError> {
        if intent == super::exact::Intent::RelatedDiscussion && !self.dense_enabled() {
            return Err(AppError::usage(
                "related-discussion requires dense gold; use exact-mention or explicitly rebuild lexical-and-dense gold",
            ));
        }
        Ok(())
    }

    pub(crate) fn generation(&self, model: &str) -> String {
        if !self.dense_enabled() {
            return "gold-lexical-v1".into();
        }
        if let Some(budget) = self.max_embedding_chars {
            let recipe = match self.embedding_text {
                EmbeddingText::Dialogue => "dialogue",
                EmbeddingText::TitleAndDialogue => "title-and-dialogue",
            };
            format!("gold-text-budget-v1:{recipe}:{}:{model}", budget.0)
        } else if self == &Self::default() {
            model.to_owned()
        } else {
            format!("gold-title-v1:{model}")
        }
    }

    pub(crate) fn prepare(&self, dialogue: String, title: Option<&str>) -> String {
        let prepared = match (
            self.embedding_text,
            title.map(str::trim).filter(|t| !t.is_empty()),
        ) {
            (EmbeddingText::TitleAndDialogue, Some(title)) if !dialogue.is_empty() => {
                // Keep metadata bounded so it cannot crowd out dialogue entirely.
                let title_limit = self
                    .max_embedding_chars
                    .map_or(256, |limit| (limit.0 as usize / 4).min(256));
                let title: String = title.chars().take(title_limit).collect();
                format!("Thread: {title}\n\n{dialogue}")
            }
            _ => dialogue,
        };
        match self.max_embedding_chars {
            Some(limit) => prepared.trim().chars().take(limit.0 as usize).collect(),
            None => prepared,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Query {
    pub(crate) intent: super::exact::Intent,
    pub(crate) unit: SearchUnit,
    pub(crate) limit: usize,
    pub(crate) fusion: Fusion,
    pub(crate) ranking: Ranking,
}

/// Query-only ranking over the active lexical and dense candidate artifacts.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    clap::ValueEnum,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Ranking {
    #[default]
    ContextualRerank,
    Fusion,
}

/// Query-only candidate fusion; independent of embedding/build identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Fusion {
    pub(crate) rank_constant: u16,
    pub(crate) lexical_weight: u16,
    pub(crate) semantic_weight: u16,
}

impl Default for Fusion {
    fn default() -> Self {
        Self {
            rank_constant: 60,
            lexical_weight: 1,
            semantic_weight: 1,
        }
    }
}

impl Fusion {
    pub(crate) fn validate(self) -> Result<(), AppError> {
        if !(1..=1000).contains(&self.rank_constant)
            || !(1..=100).contains(&self.lexical_weight)
            || !(1..=100).contains(&self.semantic_weight)
        {
            return Err(AppError::configuration(
                "gold fusion rank_constant must be 1..1000 and weights 1..100",
            ));
        }
        Ok(())
    }
}

/// Partial request policy; omitted fields retain the current index defaults.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FusionOverrides {
    /// Rank constant, from 1 through 1000.
    #[schemars(range(min = 1, max = 1000))]
    pub(crate) rank_constant: Option<u16>,
    /// Lexical candidate weight, from 1 through 100.
    #[schemars(range(min = 1, max = 100))]
    pub(crate) lexical_weight: Option<u16>,
    /// Semantic candidate weight, from 1 through 100.
    #[schemars(range(min = 1, max = 100))]
    pub(crate) semantic_weight: Option<u16>,
}

impl FusionOverrides {
    pub(crate) fn is_empty(self) -> bool {
        self == Self::default()
    }

    pub(crate) fn resolve(self, defaults: Fusion) -> Result<Fusion, AppError> {
        let resolved = Fusion {
            rank_constant: self.rank_constant.unwrap_or(defaults.rank_constant),
            lexical_weight: self.lexical_weight.unwrap_or(defaults.lexical_weight),
            semantic_weight: self.semantic_weight.unwrap_or(defaults.semantic_weight),
        };
        resolved.validate().map_err(|_| {
            AppError::usage("search fusion rank_constant must be 1..1000 and weights 1..100")
        })?;
        Ok(resolved)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Options {
    pub(crate) limit: usize,
    pub(crate) unit: SearchUnit,
    pub(crate) fusion: Fusion,
    pub(crate) ranking: Ranking,
}

impl Options {
    pub(crate) fn new(limit: usize, unit: SearchUnit) -> Self {
        Self {
            limit,
            unit,
            fusion: Fusion::default(),
            ranking: Ranking::default(),
        }
    }
}

impl Default for Query {
    fn default() -> Self {
        Self {
            intent: super::exact::Intent::default(),
            unit: SearchUnit::Threads,
            limit: 20,
            fusion: Fusion::default(),
            ranking: Ranking::default(),
        }
    }
}

impl Configuration {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if !self.build.dense_enabled()
            && self.query.intent == super::exact::Intent::RelatedDiscussion
        {
            return Err(AppError::configuration(
                "lexical-only gold requires exact-mention as the default query intent",
            ));
        }
        if !(1..=200).contains(&self.query.limit) {
            return Err(AppError::configuration(
                "gold query limit must be between 1 and 200",
            ));
        }
        self.query.fusion.validate()
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
    fn representation_selection_is_strict_and_query_capabilities_are_explicit() {
        let mut config: Configuration = toml::from_str(
            "[build]\nrepresentations='lexical-only'\n[query]\nintent='exact-mention'",
        )
        .unwrap();
        config.validate().unwrap();
        assert!(!config.build.dense_enabled());
        assert_eq!(config.build.expected_embeddings(100), 0);
        assert_eq!(config.build.generation("unused-model"), "gold-lexical-v1");
        assert!(
            config
                .build
                .require_intent(super::super::exact::Intent::RelatedDiscussion)
                .is_err()
        );
        config.query.intent = super::super::exact::Intent::RelatedDiscussion;
        assert!(config.validate().is_err());
        assert!(toml::from_str::<Configuration>("[build]\nrepresentations='dense-ish'").is_err());
        assert!(
            !serde_json::to_string(&Build::default())
                .unwrap()
                .contains("representations")
        );
    }

    #[test]
    fn ranking_is_strict_query_only_policy() {
        let defaults: Configuration = toml::from_str("").unwrap();
        let fused: Configuration = toml::from_str("[query]\nranking='fusion'").unwrap();
        assert_eq!(defaults.query.ranking, Ranking::ContextualRerank);
        assert_eq!(fused.query.ranking, Ranking::Fusion);
        assert_eq!(defaults.build, fused.build);
        assert_eq!(
            defaults.build.generation("model"),
            fused.build.generation("model")
        );
        for text in [
            "[query]\nranking='unknown'",
            "[query]\nranking=true",
            "[build]\nranking='fusion'",
        ] {
            assert!(
                toml::from_str::<Configuration>(text).is_err(),
                "accepted {text}"
            );
        }
    }

    #[test]
    fn embedding_text_budgets_are_strict_unicode_safe_and_versioned() {
        let default = Build::default();
        assert_eq!(default.generation("model"), "model");
        assert_eq!(
            serde_json::to_string(&default).unwrap(),
            r#"{"embedding_text":"dialogue"}"#
        );
        let mut limited: Build = toml::from_str("max_embedding_chars=256").unwrap();
        let source = format!("  {} trailing evidence", "é".repeat(300));
        assert_eq!(limited.prepare(source.clone(), None), "é".repeat(256));
        assert_ne!(limited.generation("model"), default.generation("model"));
        let identity = limited.generation("model");
        limited.embedding_text = EmbeddingText::TitleAndDialogue;
        assert_ne!(limited.generation("model"), identity);
        let prepared = limited.prepare(source, Some(&"title".repeat(100)));
        assert_eq!(prepared.chars().count(), 256);
        assert!(prepared.contains('é'));
        assert!(prepared.starts_with("Thread: "));
        assert_eq!(limited.prepare(String::new(), Some("secret")), "");
        limited.max_embedding_chars = Some(512_u32.try_into().unwrap());
        assert_ne!(limited.generation("model"), identity);
        assert_eq!(
            serde_json::from_str::<Build>(&serde_json::to_string(&limited).unwrap()).unwrap(),
            limited
        );
        for value in ["0", "255", "65537", "-1", "1.5", "true", "\"256\""] {
            assert!(toml::from_str::<Build>(&format!("max_embedding_chars={value}")).is_err());
            assert!(
                serde_json::from_str::<Build>(&format!("{{\"max_embedding_chars\":{value}}}"))
                    .is_err()
            );
        }
        assert!(toml::from_str::<Build>("max_embedding_chars=65536").is_ok());
    }

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

    #[test]
    fn partial_fusion_overrides_inherit_defaults_and_reject_invalid_requests() {
        let schema = serde_json::to_value(schemars::schema_for!(FusionOverrides)).unwrap();
        for (field, maximum) in [
            ("rank_constant", 1000),
            ("lexical_weight", 100),
            ("semantic_weight", 100),
        ] {
            assert_eq!(schema["properties"][field]["minimum"], 1);
            assert_eq!(schema["properties"][field]["maximum"], maximum);
        }
        let defaults = Fusion {
            rank_constant: 10,
            lexical_weight: 4,
            semantic_weight: 2,
        };
        let input: FusionOverrides = serde_json::from_str(r#"{"semantic_weight":7}"#).unwrap();
        assert_eq!(
            input.resolve(defaults).unwrap(),
            Fusion {
                semantic_weight: 7,
                ..defaults
            }
        );
        assert_eq!(
            FusionOverrides::default().resolve(defaults).unwrap(),
            defaults
        );
        for json in [
            r#"{"rank_constant":0}"#,
            r#"{"rank_constant":1001}"#,
            r#"{"lexical_weight":0}"#,
            r#"{"semantic_weight":101}"#,
        ] {
            let input: FusionOverrides = serde_json::from_str(json).unwrap();
            assert!(matches!(
                input.resolve(defaults).unwrap_err().error.kind,
                crate::ErrorKind::Usage
            ));
        }
        for json in [
            r#"{"unknown":1}"#,
            r#"{"lexical_weight":-1}"#,
            r#"{"semantic_weight":1.5}"#,
        ] {
            assert!(serde_json::from_str::<FusionOverrides>(json).is_err());
        }
    }
}
