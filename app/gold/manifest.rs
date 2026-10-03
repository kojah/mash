//! Identity of the single published gold projection; query defaults are excluded.
use super::config::Build;
use crate::silver::certificate::Certificate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub(crate) schema_version: u32,
    pub(crate) build: Build,
    pub(crate) model_generation: String,
    pub(crate) silver: Certificate,
}

impl Manifest {
    pub(crate) fn new(build: Build, model_generation: String, silver: Certificate) -> Self {
        Self {
            schema_version: 1,
            build,
            model_generation,
            silver,
        }
    }

    pub(crate) fn matches(&self, build: &Build, model: &str, silver: &Certificate) -> bool {
        self.schema_version == 1
            && &self.build == build
            && self.model_generation == model
            && self.silver.is_complete()
            && self.silver.revision == silver.revision
            && self.silver.interpretation_version == silver.interpretation_version
            && silver.interpretation_version == crate::silver::certificate::INTERPRETATION_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::silver::certificate::{Completeness, INTERPRETATION_VERSION};

    #[test]
    fn identity_checks_recipe_model_schema_revision_and_certified_input() {
        let silver = Certificate {
            revision: 42,
            interpretation_version: INTERPRETATION_VERSION,
            completeness: Completeness::Complete,
            rejected_sources: 0,
        };
        let manifest = Manifest::new(Build::default(), "model".into(), silver.clone());
        assert!(manifest.matches(&Build::default(), "model", &silver));
        assert!(!manifest.matches(&Build::default(), "another-model", &silver));
        assert!(!manifest.matches(
            &Build {
                max_embedding_chars: None,
                representations: crate::gold::config::Representations::LexicalAndDense,
                dense_content: crate::gold::config::DenseContent::AllVisible,
                text_preparation: crate::gold::config::TextPreparation::Preserve,
                embedding_text: super::super::config::EmbeddingText::TitleAndDialogue
            },
            "model",
            &silver
        ));
        let mut changed = silver.clone();
        changed.revision += 1;
        assert!(!manifest.matches(&Build::default(), "model", &changed));
        changed = silver.clone();
        changed.interpretation_version += 1;
        assert!(!manifest.matches(&Build::default(), "model", &changed));
        let mut changed_manifest = manifest.clone();
        changed_manifest.schema_version += 1;
        assert!(!changed_manifest.matches(&Build::default(), "model", &silver));
        changed_manifest = manifest.clone();
        changed_manifest.silver.completeness = Completeness::Unverified;
        assert!(!changed_manifest.matches(&Build::default(), "model", &silver));
        // Rejected refreshes with no canonical changes may resume the already
        // certified projection; they cannot certify a new build.
        changed = silver;
        changed.completeness = Completeness::Incomplete;
        assert!(manifest.matches(&Build::default(), "model", &changed));
    }
}
