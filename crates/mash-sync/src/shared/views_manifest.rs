//! View build evidence reported for one node's current silver revision.
use super::silver_revision::State;
use crate::views::config::Build;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SharedViewsManifest")]
pub struct Manifest {
    pub(super) schema_version: u32,
    pub(super) interpretation_version: u32,
    pub build: Build,
    pub(super) model_generation: String,
    pub silver: State,
}

impl Manifest {
    pub(super) fn new(build: Build, model_generation: String, silver: State) -> Self {
        Self {
            schema_version: 1,
            interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
            build,
            model_generation,
            silver,
        }
    }

    pub(super) fn matches(&self, build: &Build, model: &str, silver: Option<&State>) -> bool {
        self.schema_version == 1
            && self.interpretation_version == crate::silver::certificate::INTERPRETATION_VERSION
            && self.silver.validate().is_ok()
            && &self.build == build
            && self.model_generation == model
            && silver == Some(&self.silver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_identity_rejects_stale_silver_model_recipe_and_schema() {
        let silver = State::next(None, "a".repeat(64)).unwrap();
        let build = Build::default();
        let manifest = Manifest::new(build.clone(), "model".into(), silver.clone());
        assert!(manifest.matches(&build, "model", Some(&silver)));
        let changed = State::next(Some(&silver), "b".repeat(64)).unwrap();
        assert!(!manifest.matches(&build, "model", Some(&changed)));
        assert!(!manifest.matches(&build, "other", Some(&silver)));
        assert!(!manifest.matches(&build, "model", None));
        let title = Build {
            max_embedding_chars: None,
            representations: crate::views::config::Representations::LexicalAndDense,
            dense_content: crate::views::config::DenseContent::AllVisible,
            retrieval_unit: crate::views::config::RetrievalUnit::Message,
            text_preparation: crate::views::config::TextPreparation::Preserve,
            embedding_text: crate::views::config::EmbeddingText::TitleAndDialogue,
        };
        assert!(!manifest.matches(&title, "model", Some(&silver)));
        let mut unknown = manifest.clone();
        unknown.schema_version = 99;
        assert!(!unknown.matches(&build, "model", Some(&silver)));
        unknown = manifest.clone();
        unknown.interpretation_version = 99;
        assert!(!unknown.matches(&build, "model", Some(&silver)));
        let roundtrip: Manifest =
            serde_json::from_str(&serde_json::to_string(&manifest).unwrap()).unwrap();
        assert_eq!(manifest, roundtrip);
    }
}
