//! Lexical artifact identity is independent of dense models and recipes.
use super::silver_revision::State;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    schema_version: u32,
    interpretation_version: u32,
    representation_version: u32,
    silver: State,
}

impl Manifest {
    pub(super) fn new(silver: State) -> Self {
        Self {
            schema_version: 1,
            interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
            representation_version: 1,
            silver,
        }
    }

    pub(super) fn matches(&self, silver: Option<&State>) -> bool {
        self.schema_version == 1
            && self.representation_version == 1
            && self.interpretation_version == crate::silver::certificate::INTERPRETATION_VERSION
            && self.silver.validate().is_ok()
            && silver == Some(&self.silver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_identity_rejects_stale_silver_and_unknown_versions() {
        let silver = State::next(None, "a".repeat(64)).unwrap();
        let manifest = Manifest::new(silver.clone());
        assert!(manifest.matches(Some(&silver)));
        assert!(!manifest.matches(None));
        let changed = State::next(Some(&silver), "b".repeat(64)).unwrap();
        assert!(!manifest.matches(Some(&changed)));
        for field in 0..3 {
            let mut unknown = manifest.clone();
            match field {
                0 => unknown.schema_version += 1,
                1 => unknown.interpretation_version += 1,
                _ => unknown.representation_version += 1,
            }
            assert!(!unknown.matches(Some(&silver)));
        }
        assert_eq!(
            serde_json::from_str::<Manifest>(&serde_json::to_string(&manifest).unwrap()).unwrap(),
            manifest
        );
    }
}
