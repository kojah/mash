//! Local lexical publication, independent of dense recipes and model assets.
use crate::silver::certificate::{Certificate, INTERPRETATION_VERSION};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub(crate) schema_version: u32,
    /// FTS5 unicode61 over interpreted dialogue, with literal verification.
    pub(crate) representation_version: u32,
    pub(crate) silver: Certificate,
    pub(crate) searchable_messages: u64,
}

impl Manifest {
    pub(crate) fn new(silver: Certificate, searchable_messages: u64) -> Self {
        Self {
            schema_version: 1,
            representation_version: 1,
            silver,
            searchable_messages,
        }
    }

    pub(crate) fn matches(&self, silver: &Certificate, searchable_messages: u64) -> bool {
        self.schema_version == 1
            && self.representation_version == 1
            && self.silver.permits_views()
            && silver.permits_views()
            && self.silver.revision == silver.revision
            && self.silver.interpretation_version == silver.interpretation_version
            && silver.interpretation_version == INTERPRETATION_VERSION
            && self.searchable_messages == searchable_messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::silver::certificate::Completeness;

    #[test]
    fn lexical_identity_checks_versions_coverage_and_certified_silver() {
        let silver = Certificate {
            revision: 9,
            interpretation_version: INTERPRETATION_VERSION,
            completeness: Completeness::Complete,
            rejected_sources: 0,
        };
        let manifest = Manifest::new(silver.clone(), 12);
        assert!(manifest.matches(&silver, 12));
        assert!(!manifest.matches(&silver, 11));
        for field in 0..4 {
            let mut changed = manifest.clone();
            match field {
                0 => changed.schema_version += 1,
                1 => changed.representation_version += 1,
                2 => changed.silver.interpretation_version += 1,
                _ => changed.silver.completeness = Completeness::Unverified,
            }
            assert!(!changed.matches(&silver, 12));
        }
        let mut changed = silver.clone();
        changed.revision += 1;
        assert!(!manifest.matches(&changed, 12));
        changed = silver;
        changed.completeness = Completeness::Incomplete;
        assert!(
            manifest.matches(&changed, 12),
            "unchanged rejected refresh may retain prior publication"
        );
        changed.rejected_sources = 1;
        let retained = Manifest::new(changed.clone(), 12);
        assert!(retained.matches(&changed, 12));
        for state in [Completeness::Pending, Completeness::Unverified] {
            let mut unfinished = changed.clone();
            unfinished.completeness = state;
            assert!(!retained.matches(&unfinished, 12));
            assert!(!Manifest::new(unfinished, 12).matches(&changed, 12));
        }
        assert_eq!(
            serde_json::from_str::<Manifest>(&serde_json::to_string(&manifest).unwrap()).unwrap(),
            manifest
        );
    }
}
