//! Compaction can append a reduced representation of already archived history.
use std::collections::{BTreeMap, BTreeSet};

use super::Ancestry;
use super::parents::Parents;
use crate::AppError;
use crate::ingestion::json::{Kind, ParentLink, Record, SystemSubtype};

#[derive(Default)]
pub(super) struct Replay {
    anchor: Option<String>,
    duplicates: BTreeSet<String>,
    changes: BTreeMap<String, ParentLink>,
}

impl Replay {
    pub(super) fn clear(&mut self) {
        self.anchor = None;
        self.duplicates.clear();
        self.changes.clear();
    }

    pub(super) fn observe(
        &mut self,
        record: &Record,
        parents: &mut Parents,
    ) -> Result<(), AppError> {
        let boundary = record.kind == Kind::System
            && record.subtype == Some(SystemSubtype::CompactBoundary)
            && record.parent == ParentLink::Root;
        let parent = record.parent.clone();
        let Some(uuid) = record.uuid.as_ref() else {
            if matches!(record.kind, Kind::User | Kind::Assistant) {
                self.clear();
            }
            return Ok(());
        };
        if let Some(known) = parents.get(uuid)? {
            self.duplicates.insert(uuid.clone());
            if let Ancestry::Known(previous) = known
                && previous != parent
            {
                self.changes
                    .entry(uuid.clone())
                    .or_insert_with(|| previous.clone());
                parents.insert(uuid, Ancestry::Ambiguous)?;
            }
            return Ok(());
        }
        // Preserve original archived ancestry only when the native boundary
        // proves this whole duplicate block is a compaction replay. Its logical
        // parent and preserved tail must both identify the preceding new event,
        // outside the replay. Payload equality is still checked by reconcile.
        let proven = boundary
            && self.anchor.as_ref().is_some_and(|anchor| {
                record.logical_parent.as_ref() == Some(anchor)
                    && !self.duplicates.contains(anchor)
                    && record
                        .compact_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.preserved_segment.as_ref())
                        .is_some_and(|segment| segment.tail == *anchor)
            });
        if proven {
            for (key, original) in &self.changes {
                parents.insert(key, Ancestry::Known(original.clone()))?;
            }
        }
        self.clear();
        self.anchor = Some(uuid.clone());
        parents.insert(uuid, Ancestry::Known(parent))?;
        Ok(())
    }
}
