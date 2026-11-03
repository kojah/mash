//! Active build recipe and independent query policy.
use crate::{
    AppError,
    search_results::{Page, SearchUnit},
};
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
    #[serde(skip_serializing_if = "RetrievalUnit::is_default")]
    pub(crate) retrieval_unit: RetrievalUnit,
    #[serde(skip_serializing_if = "DenseContent::is_default")]
    pub(crate) dense_content: DenseContent,
    #[serde(skip_serializing_if = "TextPreparation::is_default")]
    pub(crate) text_preparation: TextPreparation,
    #[serde(skip_serializing_if = "Representations::is_default")]
    pub(crate) representations: Representations,
    pub(crate) embedding_text: EmbeddingText,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_embedding_chars: Option<EmbeddingTextBudget>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RetrievalUnit {
    #[default]
    Message,
    ParentAndMessage,
}

impl RetrievalUnit {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_default(&self) -> bool {
        *self == Self::Message
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DenseContent {
    #[default]
    AllVisible,
    UserMessages,
}

impl DenseContent {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_default(&self) -> bool {
        *self == Self::AllVisible
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TextPreparation {
    #[default]
    Preserve,
    CollapseWhitespace,
}

impl TextPreparation {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_default(&self) -> bool {
        *self == Self::Preserve
    }

    fn prepare(self, text: String) -> String {
        if self == Self::Preserve {
            return text;
        }
        let mut prepared = String::with_capacity(text.len());
        for word in text.split_whitespace() {
            if !prepared.is_empty() {
                prepared.push(' ');
            }
            prepared.push_str(word);
        }
        prepared
    }
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
    pub(crate) fn includes_dense(&self, role: &crate::role::Role, projection: &str) -> bool {
        self.dense_enabled()
            && !projection.is_empty()
            && (self.dense_content == DenseContent::AllVisible || *role == crate::role::Role::User)
    }

    pub(crate) fn users_only(&self) -> bool {
        self.dense_content == DenseContent::UserMessages
    }
    pub(crate) fn dense_enabled(&self) -> bool {
        self.representations == Representations::LexicalAndDense
    }

    pub(crate) fn expected_embeddings(&self, eligible_messages: u64) -> u64 {
        if self.dense_enabled() {
            eligible_messages
        } else {
            0
        }
    }

    pub(crate) fn require_intent(&self, intent: super::exact::Intent) -> Result<(), AppError> {
        if intent == super::exact::Intent::RelatedDiscussion && !self.dense_enabled() {
            return Err(AppError::usage(
                "related-discussion needs semantic search, which this configuration turns off (word-exact search only); search with --intent exact-mention, or run `mash models install` and then `mash init --force` to turn it on",
            ));
        }
        Ok(())
    }

    pub(crate) fn generation(&self, model: &str) -> String {
        // The context title prefix is part of the embedding input. Version it
        // independently so every title-bearing recipe rejects old vectors.
        if self.embedding_text == EmbeddingText::TitleAndDialogue {
            let mut base = self.clone();
            base.embedding_text = EmbeddingText::Dialogue;
            return format!("views-chat-title-v2:{}", base.generation(model));
        }
        if !self.dense_enabled() {
            return "views-lexical-v1".into();
        }
        if self.retrieval_unit == RetrievalUnit::ParentAndMessage {
            let mut anchor = self.clone();
            anchor.retrieval_unit = RetrievalUnit::Message;
            return format!("views-parent-unit-v1:{}", anchor.generation(model));
        }
        if self.users_only() {
            let text = match self.embedding_text {
                EmbeddingText::Dialogue => "dialogue",
                EmbeddingText::TitleAndDialogue => "title-and-dialogue",
            };
            let preparation = match self.text_preparation {
                TextPreparation::Preserve => "preserve",
                TextPreparation::CollapseWhitespace => "collapse-whitespace",
            };
            return format!(
                "views-users-v1:{text}:{preparation}:{}:{model}",
                self.max_embedding_chars.map_or(0, |b| b.0)
            );
        }
        if self.text_preparation == TextPreparation::CollapseWhitespace {
            let recipe = match self.embedding_text {
                EmbeddingText::Dialogue => "dialogue",
                EmbeddingText::TitleAndDialogue => "title-and-dialogue",
            };
            return format!(
                "views-whitespace-v1:{recipe}:{}:{model}",
                self.max_embedding_chars.map_or(0, |b| b.0)
            );
        }
        if let Some(budget) = self.max_embedding_chars {
            let recipe = match self.embedding_text {
                EmbeddingText::Dialogue => "dialogue",
                EmbeddingText::TitleAndDialogue => "title-and-dialogue",
            };
            format!("views-text-budget-v1:{recipe}:{}:{model}", budget.0)
        } else if self == &Self::default() {
            model.to_owned()
        } else {
            format!("views-chat-title-v2:{model}")
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
                format!("Chat: {title}\n\n{dialogue}")
            }
            _ => dialogue,
        };
        let prepared = self.text_preparation.prepare(prepared);
        match self.max_embedding_chars {
            Some(limit) => prepared.trim().chars().take(limit.0 as usize).collect(),
            None => prepared,
        }
    }

    pub(crate) fn prepare_unit(
        &self,
        mut dialogue: String,
        title: Option<&str>,
        parent: Option<&str>,
    ) -> String {
        if self.retrieval_unit == RetrievalUnit::ParentAndMessage
            && !dialogue.is_empty()
            && let Some(parent) = parent.filter(|text| !text.is_empty())
        {
            dialogue.push_str("\n\nParent message:\n");
            dialogue.extend(parent.chars().take(super::units::PARENT_CHARS));
        }
        self.prepare(dialogue, title)
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
    /// How much newer messages move up the final order, as a percentage of
    /// relevance's weight; 0 ranks by relevance alone.
    pub(crate) recency_weight: u16,
}

impl Default for Fusion {
    fn default() -> Self {
        Self {
            rank_constant: 60,
            lexical_weight: 1,
            semantic_weight: 1,
            recency_weight: 50,
        }
    }
}

const FUSION_LIMITS: &str =
    "fusion rank_constant must be 1..1000, weights 1..100 and recency_weight 0..100";

impl Fusion {
    pub(crate) fn validate(self) -> Result<(), AppError> {
        if !(1..=1000).contains(&self.rank_constant)
            || !(1..=100).contains(&self.lexical_weight)
            || !(1..=100).contains(&self.semantic_weight)
            || self.recency_weight > 100
        {
            return Err(AppError::configuration(format!("views {FUSION_LIMITS}")));
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
    /// Recency's weight as a percentage of relevance, from 0 (off) through 100.
    #[schemars(range(min = 0, max = 100))]
    pub(crate) recency_weight: Option<u16>,
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
            recency_weight: self.recency_weight.unwrap_or(defaults.recency_weight),
        };
        resolved
            .validate()
            .map_err(|_| AppError::usage(format!("search {FUSION_LIMITS}")))?;
        Ok(resolved)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Options {
    pub(crate) shared_vector_search: Option<super::vector::Search>,
    pub(crate) unit: SearchUnit,
    pub(crate) fusion: Fusion,
    pub(crate) ranking: Ranking,
    /// Results leave through CLI or MCP output under this policy.
    pub(crate) output_redaction: crate::redaction::Policy,
}

impl Options {
    pub(crate) fn new(unit: SearchUnit) -> Self {
        Self {
            shared_vector_search: None,
            unit,
            fusion: Fusion::default(),
            ranking: Ranking::default(),
            output_redaction: crate::redaction::Policy::ON,
        }
    }
}

impl Default for Query {
    fn default() -> Self {
        Self {
            intent: super::exact::Intent::default(),
            unit: SearchUnit::Chats,
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
                "lexical-only views require exact-mention as the default query intent",
            ));
        }
        if !(1..=Page::MAX_LIMIT).contains(&self.query.limit) {
            return Err(AppError::configuration(
                "views query limit must be between 1 and 200",
            ));
        }
        self.query.fusion.validate()
    }
}

impl Query {
    /// The requested page of the unit's ranked results, with configured defaults.
    pub(crate) fn resolve(
        &self,
        limit: Option<usize>,
        offset: usize,
        unit: Option<SearchUnit>,
    ) -> Result<(Page, SearchUnit), AppError> {
        let unit = unit.unwrap_or(self.unit);
        Ok((Page::new(limit.unwrap_or(self.limit), offset, unit)?, unit))
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
        assert_eq!(config.build.generation("unused-model"), "views-lexical-v1");
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
    fn dense_content_selects_known_user_role_without_changing_other_artifacts() {
        let recipe: Build = toml::from_str("dense_content='user-messages'").unwrap();
        assert!(recipe.includes_dense(&crate::role::Role::User, "visible"));
        assert!(!recipe.includes_dense(&crate::role::Role::User, ""));
        for role in [
            crate::role::Role::Assistant,
            crate::role::Role::System,
            crate::role::Role::Developer,
            crate::role::Role::Tool,
            crate::role::Role::Other("reviewer".into()),
        ] {
            assert!(!recipe.includes_dense(&role, "visible"));
            assert!(Build::default().includes_dense(&role, "visible"));
        }
        assert_ne!(
            recipe.generation("model"),
            Build::default().generation("model")
        );
        assert!(toml::from_str::<Build>("dense_content='assistant'").is_err());
        assert!(toml::from_str::<Build>("dense_content=true").is_err());
    }

    #[test]
    fn whitespace_preparation_is_opt_in_unicode_safe_and_has_distinct_build_identity() {
        let source = "  alpha\t\nβ\u{a0}gamma  ".to_owned();
        assert_eq!(Build::default().prepare(source.clone(), None), source);
        let mut recipe: Build = toml::from_str("text_preparation='collapse-whitespace'").unwrap();
        assert_eq!(recipe.prepare(source, None), "alpha β gamma");
        assert_eq!(recipe.prepare(" \n\t".into(), None), "");
        assert_eq!(recipe.prepare(String::new(), Some("hidden")), "");
        let mut identities = std::collections::BTreeSet::new();
        for preparation in [
            TextPreparation::Preserve,
            TextPreparation::CollapseWhitespace,
        ] {
            for text in [EmbeddingText::Dialogue, EmbeddingText::TitleAndDialogue] {
                for budget in [None, Some(256_u32.try_into().unwrap())] {
                    recipe.text_preparation = preparation;
                    recipe.embedding_text = text;
                    recipe.max_embedding_chars = budget;
                    assert!(identities.insert(recipe.generation("model")));
                    assert_eq!(
                        serde_json::from_str::<Build>(&serde_json::to_string(&recipe).unwrap())
                            .unwrap(),
                        recipe
                    );
                }
            }
        }
        recipe.text_preparation = TextPreparation::CollapseWhitespace;
        assert_eq!(
            recipe.prepare(" α\nβ ".into(), Some("a\tb")),
            "Chat: a b α β"
        );
        assert_eq!(
            recipe.prepare(" é\t".repeat(300), None).chars().count(),
            256
        );
        for value in ["'unknown'", "true", "7"] {
            assert!(toml::from_str::<Build>(&format!("text_preparation={value}")).is_err());
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
        assert!(prepared.starts_with("Chat: "));
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
    fn renamed_chat_title_invalidates_all_previous_title_embedding_recipes() {
        for options in [
            "",
            "dense_content='user-messages'",
            "text_preparation='collapse-whitespace'",
            "max_embedding_chars=256",
            "retrieval_unit='parent-and-message'",
        ] {
            let recipe: Build =
                toml::from_str(&format!("embedding_text='title-and-dialogue'\n{options}")).unwrap();
            assert!(
                recipe
                    .generation("model")
                    .starts_with("views-chat-title-v2:")
            );
            assert!(
                recipe
                    .prepare("body".into(), Some("title"))
                    .starts_with("Chat: title")
            );
        }
        assert_eq!(Build::default().generation("model"), "model");
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
            "Chat: title\n\nbody"
        );
        for title in [None, Some("  ")] {
            assert_eq!(build.prepare("body".into(), title), "body");
        }
        assert_eq!(build.prepare(String::new(), Some("secret")), "");
        let title = "é".repeat(300);
        assert_eq!(
            build.prepare("body".into(), Some(&title)),
            format!("Chat: {}\n\nbody", "é".repeat(256))
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
            recency_weight: 30,
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
