//! Seal legacy writers before adopting their acknowledged node references.
use super::super::{
    AppError, Identity, NodeHead, PointKind, Store, api, collection, failure, point_id,
};
use super::{Configuration, Phase, conflict};
use std::collections::BTreeMap;

impl Store {
    pub(super) async fn migrate_catalogue(
        &self,
        identity: &Identity,
        previous: Configuration,
        activate: bool,
    ) -> Result<String, AppError> {
        if !activate {
            return Err(AppError::configuration(
                "shared publication schema requires migration; restart with --activate-gold",
            ));
        }
        let sealed = if previous.schema_version == 1 {
            let sealed = Configuration {
                kind: PointKind::Configuration,
                schema_version: 2,
                version: super::super::publish::staging_version()?,
                publication: Some(super::super::publish::staging_version()?),
                phase: Some(Phase::Migrating),
                nodes: Some(BTreeMap::new()),
                build: self.build.clone(),
                generation: identity.generation.clone(),
                dimensions: identity.dimensions,
                vector_search: previous.vector_search,
            };
            let result = self
                .replace_configuration(identity, sealed, Some(&previous))
                .await;
            let observed = self
                .group_configuration(identity)
                .await?
                .ok_or_else(failure)?;
            if !observed.matches(self, identity) {
                result?;
                return Err(conflict("concurrent catalogue migration"));
            }
            observed
        } else {
            previous
        };
        if !sealed.matches(self, identity) {
            return Err(AppError::configuration(
                "resume migration using its selected gold settings",
            ));
        }
        if sealed.phase == Some(Phase::Ready) {
            return Ok(sealed.version);
        }
        // Authority-aware v1 APIs now reject the sealed schema. Writes already
        // issued may finish, but their post-CAS authority check cannot acknowledge.
        let nodes = self.legacy_heads(identity).await?;
        let current = self.legacy_heads(identity).await?;
        if !nodes
            .iter()
            .map(|(id, n)| (id, &n.version))
            .eq(current.iter().map(|(id, n)| (id, &n.version)))
        {
            return Err(conflict(
                "legacy publication changed during migration; retry startup",
            ));
        }
        let mut proposed = sealed.clone();
        proposed.phase = Some(Phase::Ready);
        proposed.publication = Some(super::super::publish::staging_version()?);
        proposed.nodes = Some(nodes);
        let result = self
            .replace_configuration(identity, proposed, Some(&sealed))
            .await;
        let observed = self.active_configuration(identity).await?;
        if observed.version != sealed.version {
            result?;
            return Err(conflict("concurrent catalogue activation"));
        }
        Ok(observed.version)
    }

    async fn legacy_heads(
        &self,
        identity: &Identity,
    ) -> Result<BTreeMap<String, NodeHead>, AppError> {
        let records = self
            .api
            .scroll::<NodeHead>(
                &collection(&identity.group_id, true),
                api::Filter {
                    must: vec![api::matches("kind", vec!["head".into()])],
                    ..api::Filter::default()
                },
            )
            .await?;
        let mut nodes = BTreeMap::new();
        for point in records {
            let node = point.payload;
            super::catalogue::validate_node(&node.node_id, &node)?;
            if point.id != point_id(&["head", &node.node_id])
                || nodes.len() >= 512
                || nodes.contains_key(&node.node_id)
            {
                return Err(failure());
            }
            nodes.insert(node.node_id.clone(), node);
        }
        Ok(nodes)
    }
}
