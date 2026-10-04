//! One conditional point selects both gold settings and all visible node heads.
use super::super::{AppError, Identity, NodeHead, PointKind, Store, failure};
use super::{Configuration, Phase, conflict};

pub(super) fn validate_node(id: &str, node: &NodeHead) -> Result<(), AppError> {
    let head = node.head.as_ref().ok_or_else(failure)?;
    let manifest = node.manifest.as_ref().ok_or_else(failure)?;
    if node.kind != PointKind::Head
        || node.node_id != id
        || id.is_empty()
        || id.len() > 4096
        || node.version.is_empty()
        || node.version.len() > 4096
        || head.version != node.version
        || head.revision < 1
        || manifest.bytes == 0
        || manifest.chunks == 0
        || manifest.chunks > manifest.bytes
    {
        return Err(failure());
    }
    if let Some(silver) = &head.silver_revision {
        silver.validate()?;
    }
    if let Some(threads) = &node.threads {
        if !matches!(
            head.snapshot_schema,
            crate::shared::snapshot::layers::Schema::Layered
        ) {
            return Err(failure());
        }
        let mut versions = std::collections::BTreeSet::new();
        if threads.len() as u64 != head.counts.conversations {
            return Err(failure());
        }
        for (id, thread) in threads {
            if id.is_empty()
                || id.len() > 4096
                || thread.version.len() != 64
                || !thread.version.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !versions.insert(&thread.version)
                || thread.version == node.version
                || thread.manifest.bytes == 0
                || thread.manifest.chunks == 0
                || thread.manifest.chunks > thread.manifest.bytes
                || thread.manifest.digest.len() != 64
            {
                return Err(failure());
            }
        }
    }
    Ok(())
}

impl Store {
    /// Rebase metadata only when another node won. Never rebase across activation
    /// or a change to this node's expected immutable snapshot.
    #[tracing::instrument(name = "qdrant_publish_head", skip_all)]
    pub(in crate::shared::qdrant_index) async fn publish_head(
        &self,
        identity: &Identity,
        node: NodeHead,
        expected: &str,
        configuration: &str,
    ) -> Result<(), AppError> {
        validate_node(&identity.node_id, &node)?;
        let mut current = self.active_configuration(identity).await?;
        if current.version != configuration {
            return Err(conflict("shared activation changed before publication"));
        }
        for _ in 0..3 {
            let nodes = current.nodes.as_ref().ok_or_else(failure)?;
            if nodes
                .get(&identity.node_id)
                .map_or("", |n| n.version.as_str())
                != expected
            {
                // No further write will be issued. The caller verifies the winner
                // and can safely reclaim this confirmed losing stage.
                return Ok(());
            }
            let mut proposed = current.clone();
            proposed
                .nodes
                .as_mut()
                .ok_or_else(failure)?
                .insert(identity.node_id.clone(), node.clone());
            proposed.publication = Some(super::super::publish::staging_version()?);
            let result = self
                .replace_configuration(identity, proposed, Some(&current))
                .await;
            let observed = self.active_configuration(identity).await?;
            if observed.version != configuration {
                return Err(conflict(
                    "shared activation changed during publication; retry indexing",
                ));
            }
            if observed
                .nodes
                .as_ref()
                .ok_or_else(failure)?
                .get(&identity.node_id)
                .is_some_and(|n| n.version == node.version)
            {
                return Ok(());
            }
            result?; // Ambiguous failures retain staging; do not issue another CAS.
            if observed.publication == current.publication {
                return Err(conflict("Qdrant catalogue publication was not observed"));
            }
            current = observed;
        }
        // Every issued request completed. The caller verifies the node pointer;
        // a nonmatching pointer is a confirmed loss, not an ambiguous write.
        Ok(())
    }
}

pub(in crate::shared::qdrant_index) fn ready_nodes(
    configuration: Configuration,
) -> Result<std::collections::BTreeMap<String, NodeHead>, AppError> {
    if configuration.phase != Some(Phase::Ready) {
        return Err(failure());
    }
    configuration.nodes.ok_or_else(failure)
}
