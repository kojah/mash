//! Shared service initialization for the sole Qdrant backend.
use super::Identity;
use super::configuration::{Configuration, env_name};
use crate::AppError;
pub(super) use crate::shared::qdrant_index::Store;
use std::collections::{HashMap, HashSet};

pub(super) async fn initialize(
    config: &Configuration,
    identities: &HashMap<blake3::Hash, Identity>,
    activate_gold: bool,
    activate_query: bool,
) -> Result<Store, AppError> {
    let groups: HashSet<_> = identities.values().map(|id| id.group_id.clone()).collect();
    let groups = groups.into_iter().collect::<Vec<_>>();
    let budget = crate::shared::snapshot_budget::Budget::new(config.max_mutation_snapshot_mib)?;
    let name = config
        .qdrant_url_env
        .as_ref()
        .ok_or_else(|| AppError::configuration("Qdrant requires qdrant_url_env"))?;
    env_name(name)?;
    let url = std::env::var(name)
        .map_err(|_| AppError::configuration(format!("set {name} to the Qdrant HTTP endpoint")))?;
    let key = config
        .qdrant_api_key_env
        .as_ref()
        .map(|name| {
            env_name(name)?;
            std::env::var(name)
                .map_err(|_| AppError::configuration(format!("set {name} to the Qdrant API key")))
        })
        .transpose()?;
    let store = Store::connect(
        &url,
        key.as_deref(),
        &groups,
        config.embeddings.dimensions,
        config
            .qdrant_max_node_mib
            .checked_mul(1024 * 1024)
            .ok_or_else(|| {
                AppError::configuration("Qdrant snapshot limit exceeds platform capacity")
            })?,
        config.gold.query.vector_search,
        config.qdrant_memory,
    )
    .await?
    .with_snapshot_budget(budget)
    .with_build(config.gold.build.clone());
    let mut selected = HashSet::new();
    for identity in identities.values() {
        if selected.insert(&identity.group_id) {
            let group_store = store.clone().with_query(
                config
                    .gold
                    .query
                    .groups
                    .get(&identity.group_id)
                    .copied()
                    .unwrap_or(config.gold.query.vector_search),
            );
            group_store.select_gold(identity, activate_gold).await?;
            group_store.select_query(identity, activate_query).await?;
        }
    }
    Ok(store)
}
