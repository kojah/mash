//! Concrete backend dispatch and connection/index initialization.
use super::Identity;
use super::configuration::{BackendChoice, Configuration, env_name};
use crate::AppError;
use crate::shared::{Freshness, backend, protocol, qdrant_index, redis_index};
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime, Timeouts};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub(super) enum Store {
    Postgres(Pool),
    Redis(redis_index::Store),
    Qdrant(qdrant_index::Store),
}
impl Store {
    fn label(&self) -> &'static str {
        match self {
            Self::Postgres(_) => "postgres",
            Self::Redis(_) => "redis",
            Self::Qdrant(_) => "qdrant",
        }
    }
    pub(super) async fn freshness(&self, id: &Identity) -> Result<Freshness, AppError> {
        match self {
            Self::Postgres(pool) => backend::freshness(pool, id).await,
            Self::Redis(store) => store.freshness(id).await,
            Self::Qdrant(store) => store.freshness(id).await,
        }
    }
    #[tracing::instrument(name = "backend_list_threads", skip_all, fields(backend = self.label()))]
    pub(super) async fn list_threads(
        &self,
        id: &Identity,
        input: crate::thread_catalog::Request,
    ) -> Result<crate::thread_catalog::Page, AppError> {
        match self {
            Self::Postgres(pool) => backend::list_threads(pool, id, input).await,
            Self::Redis(store) => store.list_threads(id, input).await,
            Self::Qdrant(store) => store.list_threads(id, input).await,
        }
    }
    pub(super) async fn status(&self, id: &Identity) -> Result<crate::storage::Counts, AppError> {
        match self {
            Self::Postgres(pool) => backend::status(pool, id).await,
            Self::Redis(store) => store.status(id).await,
            Self::Qdrant(store) => store.status(id).await,
        }
    }
    pub(super) async fn state(&self, id: &Identity) -> Result<protocol::NodeState, AppError> {
        match self {
            Self::Postgres(pool) => backend::state(pool, id).await,
            Self::Redis(store) => store.state(id).await,
            Self::Qdrant(store) => store.state(id).await,
        }
    }
    #[tracing::instrument(name = "backend_search", skip_all, fields(backend = self.label()))]
    pub(super) async fn search(
        &self,
        id: &Identity,
        input: protocol::Search,
    ) -> Result<protocol::Candidates, AppError> {
        match self {
            Self::Postgres(pool) => backend::search(pool, id, input).await,
            Self::Redis(store) => store.search(id, input).await,
            Self::Qdrant(store) => store.search(id, input).await,
        }
    }
    #[tracing::instrument(name = "backend_view", skip_all, fields(backend = self.label()))]
    pub(super) async fn view(
        &self,
        id: &Identity,
        input: protocol::View,
    ) -> Result<Vec<crate::storage::Message>, AppError> {
        match self {
            Self::Postgres(pool) => {
                backend::view(
                    pool,
                    id,
                    &input.node_id,
                    &input.id,
                    input.context,
                    input.content,
                )
                .await
            }
            Self::Redis(store) => store.view(id, input).await,
            Self::Qdrant(store) => store.view(id, input).await,
        }
    }
    pub(super) async fn forget(
        &self,
        id: &Identity,
        input: protocol::Forget,
    ) -> Result<protocol::Forgotten, AppError> {
        match self {
            Self::Postgres(pool) => backend::forget(pool, id, input).await,
            Self::Redis(store) => store.forget(id, input).await,
            Self::Qdrant(store) => store.forget(id, input).await,
        }
    }
    #[tracing::instrument(name = "backend_index", skip_all, fields(backend = self.label()))]
    pub(super) async fn index(
        &self,
        id: &Identity,
        input: tempfile::NamedTempFile,
    ) -> Result<protocol::Indexed, AppError> {
        match self {
            Self::Postgres(pool) => backend::index(pool, id, input).await,
            Self::Redis(store) => store.index(id, input).await,
            Self::Qdrant(store) => store.index(id, input).await,
        }
    }
}
fn pool(config: &Configuration) -> Result<Pool, AppError> {
    let name = config
        .database_url_env
        .as_ref()
        .ok_or_else(|| AppError::configuration("PostgreSQL requires database_url_env"))?;
    env_name(name)?;
    let url = std::env::var(name).map_err(|_| {
        AppError::configuration(format!(
            "set {name} to the server PostgreSQL connection string"
        ))
    })?;
    let mut options: tokio_postgres::Config = url
        .parse()
        .map_err(|_| AppError::configuration("invalid server PostgreSQL connection string"))?;
    if options.get_ssl_mode() != tokio_postgres::config::SslMode::Disable {
        options.ssl_mode(tokio_postgres::config::SslMode::Require);
    }
    options.connect_timeout(Duration::from_secs(5));
    options.tcp_user_timeout(Duration::from_secs(30));
    options.application_name("mash-server");
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| AppError::configuration("could not configure PostgreSQL TLS"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let manager = Manager::from_config(
        options,
        tokio_postgres_rustls::MakeRustlsConnect::new(tls),
        ManagerConfig {
            recycling_method: RecyclingMethod::Verified,
        },
    );
    Pool::builder(manager)
        .max_size(config.max_connections)
        .runtime(Runtime::Tokio1)
        .timeouts(Timeouts {
            wait: Some(Duration::from_secs(5)),
            create: Some(Duration::from_secs(5)),
            recycle: Some(Duration::from_secs(5)),
        })
        .build()
        .map_err(|_| AppError::configuration("could not configure PostgreSQL pool"))
}
pub(super) async fn initialize(
    config: &Configuration,
    identities: &HashMap<blake3::Hash, Identity>,
) -> Result<Store, AppError> {
    let groups: HashSet<_> = identities.values().map(|id| id.group_id.clone()).collect();
    let groups = groups.into_iter().collect::<Vec<_>>();
    let store = match config.backend {
        BackendChoice::Qdrant => {
            let name = config
                .qdrant_url_env
                .as_ref()
                .ok_or_else(|| AppError::configuration("Qdrant requires qdrant_url_env"))?;
            env_name(name)?;
            let url = std::env::var(name).map_err(|_| {
                AppError::configuration(format!("set {name} to the Qdrant HTTP endpoint"))
            })?;
            let key = config
                .qdrant_api_key_env
                .as_ref()
                .map(|name| {
                    env_name(name)?;
                    std::env::var(name).map_err(|_| {
                        AppError::configuration(format!("set {name} to the Qdrant API key"))
                    })
                })
                .transpose()?;
            Store::Qdrant(
                qdrant_index::Store::connect(
                    &url,
                    key.as_deref(),
                    &groups,
                    config.embeddings.dimensions,
                    config
                        .qdrant_max_node_mib
                        .checked_mul(1024 * 1024)
                        .ok_or_else(|| {
                            AppError::configuration(
                                "Qdrant snapshot limit exceeds platform capacity",
                            )
                        })?,
                    config.qdrant_vector_index,
                )
                .await?,
            )
        }
        BackendChoice::Postgres => {
            if config.redis_url_env.is_some() {
                return Err(AppError::configuration(
                    "PostgreSQL backend does not accept redis_url_env",
                ));
            }
            let pool = pool(config)?;
            backend::initialize(&pool, &groups).await?;
            Store::Postgres(pool)
        }
        BackendChoice::Redis => {
            if config.database_url_env.is_some() {
                return Err(AppError::configuration(
                    "Redis backend does not accept database_url_env",
                ));
            }
            let name = config
                .redis_url_env
                .as_ref()
                .ok_or_else(|| AppError::configuration("Redis requires redis_url_env"))?;
            env_name(name)?;
            let url = std::env::var(name).map_err(|_| {
                AppError::configuration(format!("set {name} to the server Redis connection string"))
            })?;
            Store::Redis(
                redis_index::Store::connect(
                    &url,
                    &groups,
                    config.embeddings.dimensions,
                    config
                        .redis_max_node_mib
                        .checked_mul(1024 * 1024)
                        .ok_or_else(|| {
                            AppError::configuration(
                                "Redis snapshot limit exceeds platform capacity",
                            )
                        })?,
                    config.redis_vector_index,
                )
                .await?,
            )
        }
    };
    if let Store::Redis(redis) = &store {
        for identity in identities.values() {
            redis.repair_metadata(identity).await?;
        }
    }
    Ok(store)
}
