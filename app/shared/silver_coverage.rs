//! Scoped interpretation coverage of retained history, separate from vectors.
use super::{protocol::ScanReport, snapshot::Data};
use crate::{
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
    data: &Data,
    revision: i64,
    snapshot: bool,
) -> Vec<Coverage> {
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
        coverage.completeness = completeness(data, &coverage.scan);
    }
    retained.into_values().collect()
}

fn completeness(data: &Data, scan: &ScanReport) -> Completeness {
    if scan.interpretation_version != INTERPRETATION_VERSION {
        return Completeness::Unverified;
    }
    if !scan.authoritative {
        return Completeness::Incomplete;
    }
    let mut mapped = HashSet::new();
    for source in data
        .sources
        .values()
        .filter(|s| s.provider == scan.provider)
    {
        if source.interpretation_version != INTERPRETATION_VERSION {
            return Completeness::Unverified;
        }
        if let Some(id) = source.conversation_id.as_deref() {
            mapped.insert(id);
        }
    }
    if data
        .conversations
        .values()
        .any(|c| c.provider == scan.provider && !mapped.contains(c.id.as_str()))
    {
        return Completeness::Unverified;
    }
    Completeness::Complete
}

pub(super) fn affected(event: &super::protocol::Event, data: &Data) -> Vec<Provider> {
    use super::protocol::{Batch, Event};
    let thread = |id: &str| data.conversations.get(id).map(|c| c.provider);
    match event {
        Event::Batch(Batch::Conversations { records }) => records
            .iter()
            .flat_map(|c| [Some(c.provider), thread(&c.id)].into_iter().flatten())
            .collect(),
        Event::Batch(Batch::Messages { records }) => records
            .iter()
            .filter_map(|m| thread(&m.conversation_id))
            .collect(),
        Event::Batch(Batch::Tombstones { records }) => records.iter().map(|t| t.provider).collect(),
        Event::Source(source) => vec![source.provider],
        Event::Seen { provider, .. } | Event::Reconcile { provider, .. } => vec![*provider],
        Event::Remove { id } => thread(id).into_iter().collect(),
        Event::Start(_) | Event::Scan(_) | Event::ReuseGold { .. } | Event::Finish => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_verified_scope_and_incomplete_discovery_are_distinct() {
        let data = Data::default();
        let mut scan = ScanReport {
            provider: Provider::Codex,
            interpretation_version: INTERPRETATION_VERSION,
            authoritative: true,
            cutoff: Some(123),
        };
        assert_eq!(completeness(&data, &scan), Completeness::Complete);
        scan.authoritative = false;
        assert_eq!(completeness(&data, &scan), Completeness::Incomplete);
        scan.interpretation_version = 0;
        assert_eq!(completeness(&data, &scan), Completeness::Unverified);
    }

    #[test]
    fn snapshots_revoke_prior_evidence_and_uncertified_edits_revoke_only_the_provider() {
        let data = Data::default();
        let reports: BTreeMap<_, _> = [Provider::Codex, Provider::ClaudeCode]
            .into_iter()
            .map(|provider| {
                (
                    provider,
                    ScanReport {
                        provider,
                        interpretation_version: INTERPRETATION_VERSION,
                        authoritative: true,
                        cutoff: None,
                    },
                )
            })
            .collect();
        let first = update(&[], &reports, &HashSet::new(), &data, 1, false);
        assert_eq!(first.len(), 2);
        let next = update(
            &first,
            &BTreeMap::new(),
            &HashSet::from([Provider::Codex]),
            &data,
            2,
            false,
        );
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].scan.provider, Provider::ClaudeCode);
        assert_eq!(next[0].publication_revision, 1);
        assert_eq!(
            update(&first, &BTreeMap::new(), &HashSet::new(), &data, 2, true),
            Vec::new()
        );
    }
}
