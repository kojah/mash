//! Scoped interpretation coverage of retained history, separate from vectors.
use super::protocol::ScanReport;
use crate::{
    AppError,
    provider::Provider,
    silver::certificate::{Completeness, INTERPRETATION_VERSION},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Coverage {
    pub(crate) scan: ScanReport,
    pub(crate) publication_revision: i64,
    pub(crate) completeness: Completeness,
}

/// Preserve untouched providers; edits without a concluding scan revoke evidence.
pub(super) fn update(
    previous: &[Coverage],
    reports: &BTreeMap<Provider, ScanReport>,
    dirty: &HashSet<Provider>,
    silver: &rusqlite::Connection,
    identity: &super::server::Identity,
    revision: i64,
    snapshot: bool,
) -> Result<Vec<Coverage>, AppError> {
    let mut retained = BTreeMap::new();
    if !snapshot {
        for coverage in previous {
            if !dirty.contains(&coverage.scan.provider) {
                retained.insert(coverage.scan.provider, coverage.clone());
            }
        }
    }
    for (&provider, scan) in reports {
        retained.insert(
            provider,
            Coverage {
                scan: scan.clone(),
                publication_revision: revision,
                completeness: Completeness::Unverified,
            },
        );
    }
    for coverage in retained.values_mut() {
        coverage.completeness = completeness(silver, identity, &coverage.scan)?;
    }
    Ok(retained.into_values().collect())
}

/// Complete when every retained thread of the provider is mapped by a current source.
fn completeness(
    silver: &rusqlite::Connection,
    identity: &super::server::Identity,
    scan: &ScanReport,
) -> Result<Completeness, AppError> {
    if scan.interpretation_version != INTERPRETATION_VERSION {
        return Ok(Completeness::Unverified);
    }
    if !scan.authoritative {
        return Ok(Completeness::Incomplete);
    }
    let gaps = super::silver_store::coverage_gaps(silver, identity, scan.provider)?;
    Ok(if gaps {
        Completeness::Unverified
    } else {
        Completeness::Complete
    })
}
