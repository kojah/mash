use super::config::{Configuration, Kind, Manager};
use super::model::{Action, Diagnostic, Machine, Service, State, compatibility, now};
use crate::core::Paths;
use futures_util::{StreamExt, stream};

fn database_schema(paths: &Paths) -> (Option<i64>, String) {
    let path = paths
        .db
        .clone()
        .unwrap_or_else(crate::core::default_database_path);
    let result = (|| {
        let connection = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(std::time::Duration::from_millis(100))?;
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
    })();
    match result {
        Ok(schema) => (Some(schema), "Opened read-only; no migration or integrity scan was run.".into()),
        Err(_) => (None, "Could not read the configured SQLite database. It may be absent, locked or inaccessible.".into()),
    }
}

pub(super) fn local_files(paths: &Paths) -> (Vec<Service>, Vec<String>) {
    let (schema, message) = database_schema(paths);
    let database = compatibility(
        schema,
        Some(crate::storage::schema::SCHEMA_VERSION),
        env!("MASH_BUILD_REVISION"),
    );
    let mut services = vec![Service {
        id: "sqlite".into(),
        name: "SQLite index".into(),
        kind: Kind::Sqlite,
        state: database.state,
        summary: if database.state == State::Healthy {
            "Readable · compatible"
        } else {
            "Schema compatibility needs attention"
        }
        .into(),
        checked_at: now(),
        pid: None,
        actions: vec![],
        control_note: "SQLite is a file, not a process. Checks do not change or migrate it.".into(),
        diagnostics: vec![Diagnostic {
            label: "Read check".into(),
            value: message,
        }],
        compatibility: Some(database),
        control_revision: None,
        logs_available: false,
    }];
    let inventory = crate::runtime::inspect(paths);
    let database_path = paths
        .db
        .clone()
        .unwrap_or_else(crate::core::default_database_path);
    let (watch_state, watch_summary) = match crate::watch::active(&database_path) {
        Ok(true) => (State::Healthy, "Watcher leadership lock is held"),
        Ok(false) => (State::Stopped, "No active watcher leader for this database"),
        Err(_) => (State::Unknown, "Watcher leadership could not be checked"),
    };
    services.push(Service { id:"watcher-leadership".into(), name:"Index watcher leadership".into(), kind:Kind::Watcher,
        state:watch_state, summary:watch_summary.into(), checked_at:now(), pid:None, actions:vec![],
        control_note:"Leadership is an observation, not a supervisor target. Configure the watcher service to enable controls.".into(),
        diagnostics:vec![Diagnostic {label:"Scope".into(),value:"A held lock proves leadership, not successful indexing or schema compatibility. Registered runtimes below report their own compatibility.".into()}],
        compatibility:None, control_revision: None, logs_available:false });
    let mut warnings = vec!["Process inventory covers registered MCP/watch processes for this database. Older releases and other databases may not be represented.".into()];
    if !inventory.complete {
        warnings.push("Some runtime records could not be inspected.".into());
    }
    for process in inventory.processes {
        let kind = match process.kind {
            crate::runtime::Kind::Mcp => Kind::Mcp,
            crate::runtime::Kind::IndexWatch => Kind::Watcher,
        };
        let compatibility = compatibility(schema, process.schema_version, &process.build_revision);
        let superseded = process.state != crate::runtime::State::Current;
        let state = match compatibility.state {
            State::Healthy if superseded => State::Degraded,
            other => other,
        };
        let summary = match state {
            State::Failed => "Running · schema mismatch",
            State::Unknown => "Running · compatibility unknown",
            State::Degraded => "Running · compatible, build differs or cannot be verified",
            _ => "Running · compatible",
        };
        services.push(Service {
            id: format!("runtime-{}", process.pid),
            name: format!("{} · PID {}", if kind == Kind::Mcp { "Mash MCP" } else { "Index watcher" }, process.pid),
            kind, state, summary: summary.into(), checked_at: now(), pid: Some(process.pid), actions: vec![],
            control_note: if kind == Kind::Mcp {
                "The agent owns this stdio process. Reconnect Mash from that agent to use an upgraded binary."
            } else {
                "This is a live runtime observation, not a supervisor target. Manage it through its configured service entry or the terminal that started it."
            }.into(),
            diagnostics: vec![], compatibility: Some(compatibility), control_revision: None, logs_available: false,
        });
    }
    (services, warnings)
}

pub(super) async fn local(config: Option<&Configuration>, paths: &Paths) -> Machine {
    let owned_paths = paths.clone();
    let (mut services, mut warnings) =
        tokio::task::spawn_blocking(move || local_files(&owned_paths))
            .await
            .unwrap_or_else(|_| (vec![], vec!["Local status worker failed.".into()]));
    services.push(Service {
        id: "dashboard".into(), name: "Dashboard".into(), kind: Kind::Dashboard,
        state: State::Healthy, summary: "Running · serving this request".into(), checked_at: now(),
        pid: Some(std::process::id()), actions: vec![],
        control_note: "This process serves the current page. Use an external supervisor to stop or restart it.".into(),
        diagnostics: vec![Diagnostic { label: "Build".into(), value: env!("MASH_BUILD_REVISION").into() }],
        compatibility: None, control_revision: None, logs_available: false,
    });
    if let Some(config) = config {
        let databases = config.databases.clone();
        match tokio::task::spawn_blocking(move || {
            databases
                .into_iter()
                .map(|database| {
                    let paths = Paths {
                        db: Some(database.path),
                        ..Paths::default()
                    };
                    let (schema, message) = database_schema(&paths);
                    let compatibility = compatibility(
                        schema,
                        Some(crate::storage::schema::SCHEMA_VERSION),
                        env!("MASH_BUILD_REVISION"),
                    );
                    Service {
                        id: format!("database-{}", database.id),
                        name: database.name,
                        kind: Kind::Sqlite,
                        state: compatibility.state,
                        summary: if compatibility.state == State::Healthy {
                            "Readable · compatible"
                        } else {
                            "Schema compatibility needs attention"
                        }
                        .into(),
                        checked_at: now(),
                        pid: None,
                        actions: vec![],
                        control_note:
                            "Read-only database check; no migration or repair is performed.".into(),
                        diagnostics: vec![Diagnostic {
                            label: "Read check".into(),
                            value: message,
                        }],
                        compatibility: Some(compatibility),
                        control_revision: None,
                        logs_available: false,
                    }
                })
                .collect::<Vec<_>>()
        })
        .await
        {
            Ok(databases) => services.extend(databases),
            Err(_) => warnings.push("Additional database checks failed.".into()),
        }
        services.extend(
            stream::iter(config.services.clone())
                .map(managed)
                .buffered(4)
                .collect::<Vec<_>>()
                .await,
        );
    } else {
        warnings.push("Service management and remote machines are not configured.".into());
    }
    let owned_paths = paths.clone();
    match tokio::task::spawn_blocking(move || crate::config::load(owned_paths.config.as_deref()))
        .await
    {
        Ok(Ok(configuration)) => services.extend(endpoints(&configuration).await),
        _ => warnings
            .push("Could not load node configuration to check Qdrant or the shared server.".into()),
    }
    Machine {
        id: config.map_or("local", |config| config.node.as_str()).into(),
        platform: std::env::consts::OS.into(),
        checked_at: now(),
        reachable: true,
        services,
        warnings,
    }
}

async fn managed(configured: super::config::Service) -> Service {
    let (state, detail) = match super::manager::status(&configured).await {
        Ok(value) => value,
        Err(message) => (State::Unknown, message),
    };
    let actions = if configured.allow_control
        && configured.kind != Kind::Dashboard
        && super::manager::supported(configured.manager)
        && state != State::Unknown
    {
        match state {
            State::Stopped | State::Failed => vec![Action::Start],
            _ if configured.manager == Manager::WindowsTask => vec![Action::Stop],
            _ => vec![Action::Stop, Action::Restart],
        }
    } else {
        vec![]
    };
    Service {
        id: format!("managed-{}", configured.id),
        name: configured.name.clone(),
        kind: configured.kind,
        state: if state == State::Healthy {
            State::Degraded
        } else {
            state
        },
        summary: match state {
            State::Healthy => "Process running · application health not established",
            State::Stopped => "Stopped",
            State::Failed => "Supervisor reports failure",
            _ => "Unknown",
        }
        .into(),
        checked_at: now(),
        pid: None,
        actions,
        control_note: if configured.allow_control {
            "Only explicitly configured supervisor actions are permitted."
        } else {
            "Management is disabled for this service in configuration."
        }
        .into(),
        diagnostics: vec![Diagnostic {
            label: "Supervisor report".into(),
            value: detail,
        }],
        compatibility: None,
        control_revision: Some(configured.control_revision()),
        logs_available: configured.log.is_some()
            || matches!(
                configured.manager,
                Manager::SystemdUser | Manager::SystemdSystem
            ),
    }
}

async fn endpoints(configuration: &crate::config::ResolvedConfig) -> Vec<Service> {
    let Ok(client) = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
    else {
        return vec![];
    };
    let mut checks = vec![(
        "qdrant".to_owned(),
        "Qdrant endpoint".to_owned(),
        Kind::Qdrant,
        client.get(format!(
            "{}/readyz",
            configuration.qdrant.url.trim_end_matches('/')
        )),
    )];
    if let Some(name) = &configuration.qdrant.api_key_env
        && let Ok(token) = std::env::var(name)
    {
        checks[0].3 = client
            .get(format!(
                "{}/readyz",
                configuration.qdrant.url.trim_end_matches('/')
            ))
            .header("api-key", token);
    }
    if let Some(shared) = &configuration.shared {
        let mut request = client
            .get(format!(
                "{}/v1/freshness",
                shared.api_url.trim_end_matches('/')
            ))
            .header("x-mash-group", &shared.group_id)
            .header("x-mash-node", &shared.node_id);
        if let Ok(token) = std::env::var(&shared.token_env) {
            request = request.bearer_auth(token);
        }
        checks.push((
            "shared-server".to_owned(),
            "Shared server endpoint".to_owned(),
            Kind::Server,
            request,
        ));
    }
    stream::iter(checks).map(|(id, name, kind, request)| async move {
        let (state, summary) = match request.send().await {
            Ok(response) if response.status().is_success() => (State::Healthy, "Readiness endpoint responded successfully".to_owned()),
            Ok(response) => (State::Degraded, format!("Endpoint responded HTTP {}; readiness not established", response.status().as_u16())),
            Err(_) => (State::Unknown, "Endpoint unreachable; this does not prove its process is stopped".into()),
        };
        Service { id, name, kind, state, summary, checked_at:now(), pid:None,
            actions:vec![], control_note:"Endpoint health is separate from supervisor state. Configure the owning process on its machine to manage it.".into(),
            diagnostics:vec![Diagnostic { label:"Scope".into(), value:"This check does not certify every collection, historical record or client runtime schema.".into() }],
            compatibility:None, control_revision: None, logs_available:false }
    }).buffered(2).collect().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_probe_reads_newer_schema_without_migrating_or_creating_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("index.db");
        let paths = Paths {
            db: Some(path.clone()),
            ..Paths::default()
        };
        assert!(database_schema(&paths).0.is_none());
        assert!(!path.exists());
        let db = rusqlite::Connection::open(&path).unwrap();
        db.pragma_update(None, "user_version", 999).unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        let (services, _) = local_files(&paths);
        assert_eq!(services[0].state, State::Failed);
        assert_eq!(
            services[0].compatibility.as_ref().unwrap().database_schema,
            Some(999)
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
