//! Compaction can append a reduced representation of already archived history.
use super::Ancestry;
use super::parents::Parents;
use crate::AppError;
use crate::ingestion::json::{Kind, ParentLink, Record, SystemSubtype};

#[derive(Default)]
pub(super) struct Replay {
    anchor: Option<String>,
    // Ambiguous marks a duplicate without saved changes; Known stores the first
    // original parent to restore only after a proven native replay boundary.
    // Both membership and changes share bounded inline/private disk storage.
    duplicates: Parents,
}

impl Replay {
    #[cfg(test)]
    pub(super) fn with_limit(limit: usize) -> Self {
        Self {
            duplicates: Parents::with_limit(limit),
            ..Self::default()
        }
    }

    pub(super) fn clear(&mut self) {
        self.anchor = None;
        self.duplicates.clear();
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
            let saved = self.duplicates.get(uuid)?;
            if let Ancestry::Known(previous) = known
                && previous != parent
            {
                if !matches!(saved, Some(Ancestry::Known(_))) {
                    self.duplicates.insert(uuid, Ancestry::Known(previous))?;
                }
                parents.insert(uuid, Ancestry::Ambiguous)?;
            } else if saved.is_none() {
                self.duplicates.insert(uuid, Ancestry::Ambiguous)?;
            }
            return Ok(());
        }
        // Preserve original archived ancestry only when the native boundary
        // proves this whole duplicate block is a compaction replay. Its logical
        // parent and preserved tail must both identify the preceding new event,
        // outside the replay. Payload equality is still checked by reconcile.
        let proven = if boundary && let Some(anchor) = self.anchor.as_ref() {
            record.logical_parent.as_ref() == Some(anchor)
                && self.duplicates.get(anchor)?.is_none()
                && record
                    .compact_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.preserved_segment.as_ref())
                    .is_some_and(|segment| segment.tail == *anchor)
        } else {
            false
        };
        if proven {
            self.duplicates.visit(|key, original| {
                if let Ancestry::Known(_) = original {
                    parents.insert(key, original)?;
                }
                Ok(())
            })?;
        }
        self.clear();
        self.anchor = Some(uuid.clone());
        parents.insert(uuid, Ancestry::Known(parent))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
