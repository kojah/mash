//! Recover recorded dialogue only across positively identified compaction.
use super::{AppError, Message, Origin, Replay, retained_bytes};
use crate::role::Role;
use std::collections::HashSet;

impl Replay {
    pub(super) fn checkpoint(&mut self, messages: Vec<Message>) -> Result<(), AppError> {
        let ids = messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<HashSet<_>>();
        // The first-party compressor emits a new summary/acknowledgement pair.
        // Rollbacks instead retain existing IDs; an arbitrary smaller checkpoint
        // is never sufficient evidence for recovering removed dialogue.
        let root = messages.windows(2).find_map(|pair| {
            if self.positions.contains_key(&pair[0].id) || self.positions.contains_key(&pair[1].id)
            {
                return None;
            }
            let (summary_role, summary) = pair[0].normalized();
            let (ack_role, ack) = pair[1].normalized();
            let text = summary.canonical.trim();
            (summary_role == Role::User
                && text.starts_with("<state_snapshot>")
                && text.ends_with("</state_snapshot>")
                && ack_role == Role::Assistant
                && ack.canonical.trim() == "Got it. Thanks for the additional context!")
                .then(|| pair[0].id.clone())
        });
        let compacting = root.is_some();
        if let Some(root) = root {
            self.compression_root = Some(root);
        } else if self
            .compression_root
            .as_ref()
            .is_some_and(|root| !ids.contains(root.as_str()))
        {
            // Removing the compaction anchor resets the context epoch. Do not
            // resurrect its archived history after a rewind/reset checkpoint.
            self.archived.clear();
            self.archived_ids.clear();
            self.compression_root = None;
        }
        let previous = std::mem::take(&mut self.positions);
        let archived_ids = std::mem::take(&mut self.archived_ids);
        // Reintroduced IDs are owned by the authoritative checkpoint, once.
        self.archived.retain(|m| !ids.contains(m.id.as_str()));
        for message in self.messages.drain(..) {
            if compacting
                && !ids.contains(message.id.as_str())
                && previous
                    .get(&message.id)
                    .is_some_and(|p| matches!(p.origin, Origin::Recorded))
            {
                self.archived.push(message);
            }
        }
        self.archived_ids = self.archived.iter().map(|m| m.id.clone()).collect();
        self.retained_bytes = self.archived.iter().map(retained_bytes).sum();
        for message in messages {
            let origin = previous.get(&message.id).map_or_else(
                || {
                    if archived_ids.contains(&message.id) {
                        Origin::Recorded
                    } else {
                        Origin::ModelContext
                    }
                },
                |p| p.origin,
            );
            self.message(message, origin)?;
        }
        Ok(())
    }
}
