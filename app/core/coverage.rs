//! Scoped retained-history facts, without claiming that unseen history is absent.
use crate::{AppError, provider::Provider};
use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema, clap::Args)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Scope {
    #[arg(long, requires = "coverage")]
    pub(crate) provider: Vec<Provider>,
    #[arg(long, requires = "coverage")]
    pub(crate) node: Vec<String>,
    /// Inclusive message timestamp in Unix seconds.
    #[arg(long, requires = "coverage")]
    pub(crate) after: Option<i64>,
    /// Exclusive message timestamp in Unix seconds.
    #[arg(long, requires = "coverage")]
    pub(crate) before: Option<i64>,
}

impl Scope {
    fn selection(&self) -> crate::search_filter::Selection {
        crate::search_filter::Selection {
            provider: self.provider.clone(),
            node: self.node.clone(),
            after: self.after,
            before: self.before,
            ..Default::default()
        }
    }
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        self.selection().resolve("local")?;
        if self.after.zip(self.before).is_some_and(|(a, b)| a >= b) {
            return Err(AppError::usage(
                "coverage after must be earlier than before",
            ));
        }
        Ok(())
    }
    pub(crate) fn includes(&self, node: &str) -> bool {
        self.node.is_empty() || self.node.iter().any(|value| value == node)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) scope: Scope,
    /// False: scan success and retained rows do not prove complete provider history.
    pub(super) absence_proven: bool,
    pub(crate) members: Vec<Member>,
    pub(crate) unavailable_nodes: Vec<String>,
    pub(super) limitations: Vec<Limitation>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Limitation {
    SourceFreshnessNotChecked,
    HistoricalScanScopeUnrecorded,
    SharedProviderScanStateUnreported,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Index {
    Local,
    Shared,
}

#[derive(Debug, Serialize)]
pub(crate) struct Member {
    pub(crate) node_id: String,
    pub(crate) index: Index,
    pub(crate) source_silver: Option<crate::silver::certificate::Certificate>,
    /// Scoped canonical row counts; null when this source does not report them.
    pub(crate) retained: Option<Retained>,
    /// Local scan evidence only; null when the publisher does not report it.
    pub(crate) providers: Option<Vec<Scan>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Retained {
    pub(crate) chats: u64,
    pub(crate) messages: u64,
    /// Observed message bounds in Unix milliseconds, not a completeness interval.
    pub(crate) earliest_message_at: Option<i64>,
    pub(crate) latest_message_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Scan {
    pub(super) provider: Provider,
    pub(super) configured: bool,
    /// Completion of a recorded scan, not completeness of all historical chats.
    pub(super) state: crate::silver::certificate::Completeness,
    pub(super) rejected_sources: u64,
}

pub(super) async fn local(
    database: &std::path::Path,
    config: &crate::config::ResolvedConfig,
    scope: Scope,
) -> Result<Report, AppError> {
    scope.validate()?;
    let database = database.to_path_buf();
    let config = config.clone();
    crate::blocking(move || {
        let node = config
            .shared
            .as_ref()
            .map_or("local", |c| c.node_id.as_str());
        let mut members = Vec::new();
        if scope.includes(node) {
            let storage = database
                .is_file()
                .then(|| crate::storage::Storage::open_silver(&database))
                .transpose()?;
            let providers = if scope.provider.is_empty() {
                Provider::ALL.to_vec()
            } else {
                scope.provider.clone()
            };
            let scans = providers
                .into_iter()
                .map(|provider| {
                    let (state, rejected_sources) = storage.as_ref().map_or(
                        Ok((crate::silver::certificate::Completeness::Unverified, 0)),
                        |storage| storage.coverage_scan(provider),
                    )?;
                    Ok(Scan {
                        provider,
                        configured: config
                            .providers
                            .get(&provider)
                            .is_some_and(|roots| !roots.is_empty()),
                        state,
                        rejected_sources,
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            members.push(Member {
                node_id: node.to_owned(),
                index: Index::Local,
                source_silver: storage
                    .as_ref()
                    .map(crate::storage::Storage::silver_certificate)
                    .transpose()?,
                retained: storage
                    .as_ref()
                    .map(|s| s.coverage_retained(&scope.selection().resolve(node)?))
                    .transpose()?,
                providers: Some(scans),
            });
        }
        let unavailable_nodes = scope
            .node
            .iter()
            .filter(|id| !members.iter().any(|m| &m.node_id == *id))
            .cloned()
            .collect();
        Ok(Report {
            scope,
            absence_proven: false,
            members,
            unavailable_nodes,
            limitations: vec![
                Limitation::SourceFreshnessNotChecked,
                Limitation::HistoricalScanScopeUnrecorded,
            ],
        })
    })
    .await
}

impl Report {
    pub(crate) fn add_shared(
        &mut self,
        freshness: &crate::shared::Freshness,
    ) -> Result<(), AppError> {
        self.members
            .extend(freshness.coverage_members(&self.scope)?);
        self.unavailable_nodes
            .retain(|id| !self.members.iter().any(|m| &m.node_id == id));
        self.limitations
            .push(Limitation::SharedProviderScanStateUnreported);
        Ok(())
    }
}
