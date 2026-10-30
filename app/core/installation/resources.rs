//! Read-only local setup evidence; never opens an incompatible schema through Storage.
use super::{Component, Finding, State};
use crate::{core::Paths, provider::Provider, storage::Storage};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub(super) struct Report {
    pub(super) findings: Vec<Finding>,
    providers: Vec<ProviderStatus>,
    configuration_loaded: bool,
    silver: Option<crate::silver::certificate::Certificate>,
    connectors: Option<Vec<crate::source::SourceStatus>>,
    connector_scope: &'static str,
    runtimes: crate::runtime::Inventory,
}

#[derive(Debug, Serialize)]
struct ProviderStatus {
    provider: Provider,
    configured: bool,
    roots: Vec<Root>,
    scan: Option<Scan>,
    next_action: &'static str,
}

#[derive(Debug, Serialize)]
struct Root {
    path: PathBuf,
    state: State,
}

#[derive(Debug, Serialize)]
struct Scan {
    state: crate::silver::certificate::Completeness,
    rejected_sources: u64,
}

pub(super) fn index_finding(database: &Path) -> Finding {
    let mut marker = database.as_os_str().to_owned();
    marker.push(".upgrade-in-progress");
    let marker = PathBuf::from(marker);
    if marker.exists() {
        return Finding {
            component: Component::Index,
            state: State::Failed,
            path: Some(marker),
            message: "A local-index upgrade marker remains. The upgrade may be running or interrupted; do not start indexing or retry installation.".into(),
            remediation: Some("Check the upgrade process. If it stopped, inspect the marker's backup file and retained/staged files; recover the complete database and attachments before clearing the marker.".into()),
        };
    }
    let (state, message, remedy) = match std::fs::metadata(database) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            State::NotConfigured,
            "No local index exists. This may be a fresh setup; missing files alone do not establish an interrupted upgrade.".into(),
            Some("For a new setup, run `mash index`. After a failed upgrade, recover the retained backup before indexing."),
        ),
        Err(_) => (State::Unverified, "Cannot inspect the local index path.".into(), Some("Check path access and permissions.")),
        Ok(metadata) if !metadata.is_file() => (State::Invalid, "The local index path is not a file.".into(), Some("Correct --db to point to the canonical SQLite file.")),
        Ok(_) => inspect_schema(database),
    };
    Finding {
        component: Component::Index,
        state,
        path: Some(database.to_path_buf()),
        message,
        remediation: remedy.map(str::to_owned),
    }
}

fn inspect_schema(database: &Path) -> (State, String, Option<&'static str>) {
    let version =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .and_then(|connection| {
                connection.busy_timeout(std::time::Duration::from_millis(100))?;
                connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            });
    match version {
        Ok(version) if version == crate::storage::schema::SCHEMA_VERSION => (
            State::Present,
            format!(
                "Local silver schema {version} matches this binary. This does not certify views or synchronization readiness."
            ),
            None,
        ),
        Ok(version) => (
            State::Outdated,
            format!(
                "Local silver schema {version} does not match this binary's schema {}.",
                crate::storage::schema::SCHEMA_VERSION
            ),
            Some(
                "Retain the database and attachments. Export with a compatible release, then import a supported portable export into a fresh database. Legacy format-1 exports require an intermediate exporter; do not retry indexing this database.",
            ),
        ),
        Err(_) => (
            State::Failed,
            "Cannot read the local SQLite schema; compatibility is unknown.".into(),
            Some(
                "Check file permissions, SQLite locks and database integrity before attempting repair.",
            ),
        ),
    }
}

pub(super) fn inspect(paths: &Paths) -> Report {
    let database = paths
        .db
        .clone()
        .unwrap_or_else(crate::core::default_database_path);
    let index = index_finding(&database);
    let compatible = index.state == State::Present;
    let mut report = Report {
        findings: vec![index],
        providers: vec![],
        configuration_loaded: false,
        silver: None,
        connectors: None,
        runtimes: crate::runtime::inspect(paths),
        connector_scope: "Local committed connector deliveries only. No delivery does not prove a connector is unconfigured. External credentials, live connections and shared-only sources are not inspected; use `mash status --shared` for shared readiness.",
    };
    add_runtime_findings(&mut report);
    let storage = if compatible {
        Storage::open_catalog(&database).ok()
    } else {
        None
    };
    inspect_catalog(&mut report, storage.as_ref(), compatible, &database);
    inspect_configuration(&mut report, paths, storage.as_ref());
    report
}

fn add_runtime_findings(report: &mut Report) {
    for process in &report.runtimes.processes {
        let state = match process.state {
            crate::runtime::State::Current => continue,
            crate::runtime::State::Superseded | crate::runtime::State::DifferentBuild => {
                State::Outdated
            }
            crate::runtime::State::Unverified => State::Unverified,
        };
        report.findings.push(Finding {
            component: Component::Runtime,
            state,
            path: None,
            message: format!("Live Mash process {} has a replaced/unverified executable or a different build from this observer. Inspect resources.runtimes for the precise state.", process.pid),
            remediation: Some("Reconnect that MCP session or restart its explicit `mash index --watch`. Replacement of its invocation path blocks new writes; an unchanged old release path does not. Already admitted work may finish.".into()),
        });
    }
    if !report.runtimes.complete {
        report.findings.push(Finding {
            component: Component::Runtime,
            state: State::Unverified,
            path: None,
            message: "Runtime records could not be completely inspected.".into(),
            remediation: Some("Check runtime-directory access. Older releases without runtime registration remain outside this inventory.".into()),
        });
    }
}

fn inspect_catalog(
    report: &mut Report,
    storage: Option<&Storage>,
    compatible: bool,
    database: &Path,
) {
    if let Some(storage) = storage {
        match storage.silver_certificate() {
            Ok(certificate) => {
                if !certificate.is_complete() {
                    report.findings.push(Finding {
                        component: Component::Index,
                        state: State::Unverified,
                        path: Some(database.to_path_buf()),
                        message: "Stored history is pending, incomplete or unverified; a completed provider scan alone does not certify all retained sources. Inspect resources.silver for the recorded state.".into(),
                        remediation: Some("Inspect `mash status --coverage` and any running index operation. After correcting provider roots or rejected sources, use an explicit `mash index --all-history` refresh; retain upgrade exports for history absent from current provider stores.".into()),
                    });
                }
                report.silver = Some(certificate);
            }
            Err(_) => report.findings.push(unverified("Cannot read the stored history certificate; source interpretation freshness is unverified.")),
        }
        match storage.connector_activity() {
            Ok(connectors) => report.connectors = Some(connectors),
            Err(_) => report.findings.push(unverified(
                "Cannot read retained connector delivery evidence.",
            )),
        }
    } else if compatible {
        report.findings.push(unverified("Schema matches, but the catalog cannot be inspected. Provider scans and connector deliveries are unverified."));
    }
}

fn inspect_configuration(report: &mut Report, paths: &Paths, storage: Option<&Storage>) {
    match crate::config::load(paths.config.as_deref()) {
        Ok(config) => {
            report.configuration_loaded = config.loaded;
            for &provider in Provider::ALL {
                let roots = config.providers.get(&provider);
                if provider == Provider::Codex {
                    for path in unconfigured_codex_stores(roots.map_or(&[], Vec::as_slice)) {
                        report.findings.push(Finding {
                            component: Component::Configuration,
                            state: State::Missing,
                            path: Some(path),
                            message: "A Codex paginated history database exists beside a configured sessions directory but is outside the configured roots. A completed JSONL scan does not cover this history.".into(),
                            remediation: Some("Add this thread_history_1.sqlite path to providers.codex.roots; its sibling state_5.sqlite is required metadata. Then run an explicit all-history refresh.".into()),
                        });
                    }
                }
                if config.loaded && roots.is_some_and(|roots| roots.iter().any(|path| root_state(path).needs_attention())) {
                    report.findings.push(Finding {
                        component: Component::Configuration,
                        state: State::Invalid,
                        path: Some(config.path.clone()),
                        message: format!("A configured {} history root is missing or is not a file/directory; inspect this provider's root states.", provider.as_str()),
                        remediation: Some("Correct the configured roots or remove the provider configuration if it is not used on this machine.".into()),
                    });
                }
                let scan = storage.as_ref().and_then(|storage| storage.coverage_scan(provider).ok()).map(|(state, rejected_sources)| Scan { state, rejected_sources });
                report.providers.push(ProviderStatus {
                    provider,
                    configured: roots.is_some_and(|roots| !roots.is_empty()),
                    roots: roots.into_iter().flatten().map(|path| Root { path: path.clone(), state: root_state(path) }).collect(),
                    scan,
                    next_action: "Review configured roots and access first; run `mash index` after setup. Recorded scan completion does not certify current source freshness. Use `mash status --coverage` for retained-history details.",
                });
            }
        }
        Err(_) => report.findings.push(Finding {
            component: Component::Configuration,
            state: State::Invalid,
            path: paths.config.clone(),
            message: "Mash configuration cannot be loaded; provider setup is unverified. Agent and index diagnostics remain available.".into(),
            remediation: Some("Correct the node TOML configuration; ordinary `mash status` provides its validation error.".into()),
        }),
    }
}

pub(super) fn setup_needs_attention(paths: &Paths) -> bool {
    let runtimes = crate::runtime::inspect(paths);
    if !runtimes.complete
        || runtimes
            .processes
            .iter()
            .any(|process| process.state != crate::runtime::State::Current)
    {
        return true;
    }
    match crate::config::load(paths.config.as_deref()) {
        Ok(config) => {
            !unconfigured_codex_stores(
                config
                    .providers
                    .get(&Provider::Codex)
                    .map_or(&[], Vec::as_slice),
            )
            .is_empty()
                || config.loaded
                    && config
                        .providers
                        .values()
                        .flatten()
                        .any(|path| root_state(path).needs_attention())
        }
        Err(_) => true,
    }
}

fn unconfigured_codex_stores(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .filter(|root| {
            root.file_name()
                .is_some_and(|name| name == "sessions" || name == "archived_sessions")
        })
        .filter_map(|root| {
            root.parent()
                .map(|parent| parent.join("thread_history_1.sqlite"))
        })
        .filter(|database| {
            database.is_file() && !roots.iter().any(|root| database.starts_with(root))
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn root_state(path: &Path) -> State {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            if std::fs::read_dir(path).is_ok() {
                State::Present
            } else {
                State::Unverified
            }
        }
        Ok(metadata) if metadata.is_file() => {
            if std::fs::File::open(path).is_ok() {
                State::Present
            } else {
                State::Unverified
            }
        }
        Ok(_) => State::Invalid,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => State::Missing,
        Err(_) => State::Unverified,
    }
}

fn unverified(message: &str) -> Finding {
    Finding {
        component: Component::Index,
        state: State::Unverified,
        path: None,
        message: message.into(),
        remediation: Some(
            "Inspect ordinary status and repair catalog access before relying on source health."
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_and_help_notice_report_a_superseded_live_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("mash");
        std::fs::write(&executable, "running").unwrap();
        let config = directory.path().join("config.toml");
        std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
        let paths = Paths {
            config: Some(config),
            db: Some(directory.path().join("index.sqlite3")),
            ..Paths::default()
        };
        let lease = crate::runtime::Lease::fixture(&paths, &executable);
        assert!(!setup_needs_attention(&paths));
        std::fs::write(&executable, "new executable").unwrap();
        let report = inspect(&paths);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| matches!(finding.component, Component::Runtime)
                    && finding.state == State::Outdated)
        );
        assert!(setup_needs_attention(&paths));
        drop(lease);
        assert!(!setup_needs_attention(&paths));
    }

    #[test]
    fn installation_reports_stale_checkpoints_despite_a_complete_provider_scan() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("silver.sqlite3");
        drop(Storage::open(&database).unwrap());
        let connection = rusqlite::Connection::open(&database).unwrap();
        let current = crate::silver::certificate::INTERPRETATION_VERSION;
        connection
            .execute(
                "UPDATE silver_state SET interpretation_version=?1",
                [current],
            )
            .unwrap();
        connection
            .execute("INSERT INTO silver_scans VALUES('codex',?1,1)", [current])
            .unwrap();
        connection.execute("INSERT INTO source_checkpoints(provider,source_key,source_path,size_bytes,modified_ns,fingerprint,interpretation_version) VALUES('codex','old','/missing/old.jsonl',1,1,'old',?1)", [current - 1]).unwrap();
        let paths = Paths {
            db: Some(database.clone()),
            config: Some(directory.path().join("absent-config.toml")),
            ..Paths::default()
        };
        let report = inspect(&paths);
        assert_eq!(
            report.silver.unwrap().completeness,
            crate::silver::certificate::Completeness::Unverified
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.state == State::Unverified
                    && finding.message.contains("retained sources"))
        );
        connection
            .execute(
                "UPDATE source_checkpoints SET interpretation_version=?1",
                [current],
            )
            .unwrap();
        let report = inspect(&paths);
        assert!(report.silver.unwrap().is_complete());
        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.message.contains("retained sources"))
        );
    }

    #[test]
    fn installation_detects_codex_history_outside_configured_jsonl_roots() {
        let directory = tempfile::tempdir().unwrap();
        let sessions = directory.path().join("sessions");
        let store = directory.path().join("thread_history_1.sqlite");
        std::fs::create_dir(&sessions).unwrap();
        assert_eq!(
            unconfigured_codex_stores(std::slice::from_ref(&sessions)),
            Vec::<PathBuf>::new()
        );
        std::fs::write(&store, []).unwrap();
        assert_eq!(
            unconfigured_codex_stores(std::slice::from_ref(&sessions)),
            std::slice::from_ref(&store)
        );
        assert_eq!(
            unconfigured_codex_stores(&[sessions, store]),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            unconfigured_codex_stores(&[directory.path().into()]),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn installation_resources_distinguish_deliveries_scans_and_live_health() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("silver.sqlite3");
        drop(Storage::open(&database).unwrap());
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection.execute_batch("INSERT INTO connector_sources VALUES('example', '1'); INSERT INTO connector_activity(source_id,modes,snapshot_at,complete_snapshot_at) VALUES('example','[\"snapshot\"]',1234,1234);").unwrap();
        connection
            .execute(
                "INSERT INTO silver_scans VALUES('codex',?1,1)",
                [crate::silver::certificate::INTERPRETATION_VERSION],
            )
            .unwrap();
        drop(connection);
        let config = directory.path().join("config.toml");
        std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
        let report = inspect(&Paths {
            db: Some(database),
            config: Some(config),
            models_dir: None,
        });
        assert_eq!(report.findings[0].state, State::Present);
        let connectors = report.connectors.unwrap();
        assert_eq!(connectors.len(), 1);
        assert_eq!(
            connectors[0].source,
            crate::source::Source::Connector("example".into())
        );
        assert_eq!(connectors[0].last_complete_snapshot_at, Some(1234));
        assert_eq!(connectors[0].last_live_at, None);
        let codex = report
            .providers
            .iter()
            .find(|p| p.provider == Provider::Codex)
            .unwrap();
        assert!(!codex.configured);
        assert_eq!(
            codex.scan.as_ref().unwrap().state,
            crate::silver::certificate::Completeness::Complete
        );
        assert_eq!(codex.scan.as_ref().unwrap().rejected_sources, 0);
        assert!(report.connector_scope.contains("live connections"));
    }
}
