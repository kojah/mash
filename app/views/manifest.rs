//! Identity of the single published views projection; query defaults are excluded.
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
            && self.silver.permits_views()
            && silver.permits_views()
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
                representations: crate::views::config::Representations::LexicalAndDense,
                dense_content: crate::views::config::DenseContent::AllVisible,
                retrieval_unit: crate::views::config::RetrievalUnit::Message,
                text_preparation: crate::views::config::TextPreparation::Preserve,
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
        // Source coverage is independent of a certified retained projection.
        changed = silver;
        changed.completeness = Completeness::Incomplete;
        assert!(manifest.matches(&Build::default(), "model", &changed));
        changed.rejected_sources = 1;
        let retained = Manifest::new(Build::default(), "model".into(), changed.clone());
        assert!(retained.matches(&Build::default(), "model", &changed));
        for state in [Completeness::Pending, Completeness::Unverified] {
            let mut unfinished = changed.clone();
            unfinished.completeness = state;
            assert!(!retained.matches(&Build::default(), "model", &unfinished));
            assert!(
                !Manifest::new(Build::default(), "model".into(), unfinished).matches(
                    &Build::default(),
                    "model",
                    &changed
                )
            );
        }
    }
}
