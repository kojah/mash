use super::*;
use crate::core::Paths;

const TOKEN: &str = "test-services-token-0123456789abcdef";

#[derive(Deserialize)]
struct Fleet {
    machines: Vec<model::Machine>,
}

async fn app(root: &Path, peers: Vec<Peer>) -> (String, tokio::task::JoinHandle<()>) {
    app_named(root, peers, "test-node").await
}

async fn app_named(
    root: &Path,
    peers: Vec<Peer>,
    name: &str,
) -> (String, tokio::task::JoinHandle<()>) {
    let config: config::Configuration =
        toml::from_str(&format!("node='{name}'\ntoken_env='UNUSED'\n")).unwrap();
    let database = root.join("silver.sqlite3");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    drop(connection);
    let node_config = root.join("config.toml");
    std::fs::write(
        &node_config,
        "version=1\nproviders={}\n[qdrant]\nurl='http://127.0.0.1:1'\n",
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = Arc::new(super::super::Dash {
        paths: Paths {
            db: Some(database),
            config: Some(node_config),
            ..Paths::default()
        },
        hosts: super::super::allowed_hosts(address),
        model_work: tokio::sync::Semaphore::new(1),
        services: Services {
            config: Some(config),
            token_hash: Some(blake3::hash(TOKEN.as_bytes())),
            peers,
            directory: Some(root.to_path_buf()),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap(),
        },
    });
    let handle = tokio::spawn(async move {
        axum::serve(listener, super::super::router(state))
            .await
            .unwrap();
    });
    (format!("http://{address}"), handle)
}

#[tokio::test]
async fn services_http_requires_auth_rejects_cross_origin_and_reports_schema() {
    let root = tempfile::tempdir().unwrap();
    let (url, server) = app(root.path(), vec![]).await;
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("{url}/api/services"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!(
                "{url}/api/services/log?machine=test-node&service=sqlite"
            ))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let request = operations::ActionRequest {
        control_revision: "fixture".into(),
        machine: "test-node".into(),
        service: "sqlite".into(),
        action: model::Action::Restart,
        request_id: "12345678-1234-1234-1234-123456789abc".into(),
    };
    assert_eq!(
        client
            .post(format!("{url}/api/services/action"))
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .post(format!("{url}/api/services/action"))
            .bearer_auth(TOKEN)
            .header("origin", "http://evil.test")
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(format!("{url}/api/services/action"))
            .bearer_auth(TOKEN)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let machine: model::Machine = client
        .get(format!("{url}/api/services/local"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(machine.id, "test-node");
    let database = machine.services.iter().find(|s| s.id == "sqlite").unwrap();
    assert_eq!(database.state, model::State::Failed);
    assert_eq!(database.actions, []);
    assert_eq!(
        database.compatibility.as_ref().unwrap().database_schema,
        Some(999)
    );
    let db = rusqlite::Connection::open(root.path().join("silver.sqlite3")).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        999
    );
    server.abort();
}

#[tokio::test]
async fn services_remote_failures_are_unknown_and_forwarding_cannot_loop() {
    let root = tempfile::tempdir().unwrap();
    let (url, server) = app(
        root.path(),
        vec![Peer {
            id: "offline".into(),
            url: "http://127.0.0.1:1".into(),
            token: TOKEN.into(),
        }],
    )
    .await;
    let client = reqwest::Client::new();
    let fleet: Fleet = client
        .get(format!("{url}/api/services"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let remote = fleet.machines.iter().find(|m| m.id == "offline").unwrap();
    assert!(!remote.reachable);
    assert!(remote.services.is_empty());
    assert_ne!(remote.warnings, [] as [String; 0]);
    assert_eq!(
        client
            .get(format!("{url}/api/services/operations?machine=offline"))
            .bearer_auth(TOKEN)
            .header("x-mash-service-forwarded", "1")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    server.abort();
}

#[tokio::test]
async fn services_remote_reports_keep_identity_and_private_credentials() {
    let remote_root = tempfile::tempdir().unwrap();
    let (remote_url, remote) = app_named(remote_root.path(), vec![], "remote-node").await;
    let root = tempfile::tempdir().unwrap();
    let (url, server) = app(
        root.path(),
        vec![Peer {
            id: "remote-node".into(),
            url: remote_url,
            token: TOKEN.into(),
        }],
    )
    .await;
    let response = reqwest::Client::new()
        .get(format!("{url}/api/services"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!response.contains(TOKEN));
    let fleet: Fleet = serde_json::from_str(&response).unwrap();
    let peer = fleet
        .machines
        .iter()
        .find(|machine| machine.id == "remote-node")
        .unwrap();
    assert!(peer.reachable);
    assert_eq!(
        peer.services
            .iter()
            .find(|s| s.id == "sqlite")
            .unwrap()
            .state,
        model::State::Failed
    );
    let history: Vec<operations::Operation> = reqwest::Client::new()
        .get(format!("{url}/api/services/operations?machine=remote-node"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history.len(), 0);
    server.abort();
    remote.abort();
}

/// Own a disposable service supplied by the test runner; never point this at a
/// real Mash unit. Run under the shared test slice with a bounded service.
#[tokio::test]
#[ignore = "requires MASH_TEST_SERVICE_UNIT naming a disposable mash-control-test-* systemd user unit"]
async fn services_real_systemd_start_stop_restart_and_receipt_replay() {
    let unit = std::env::var("MASH_TEST_SERVICE_UNIT").expect("disposable unit required");
    assert!(unit.starts_with("mash-control-test-") && unit.ends_with(".service"));
    let service = config::Service {
        interrupt_after_command: false,
        id: "fixture".into(),
        name: "Disposable test process".into(),
        kind: config::Kind::Watcher,
        manager: config::Manager::SystemdUser,
        unit,
        plist: None,
        log: None,
        allow_control: true,
    };
    let root = tempfile::tempdir().unwrap();
    for (index, action) in [
        model::Action::Stop,
        model::Action::Start,
        model::Action::Restart,
        model::Action::Stop,
    ]
    .into_iter()
    .enumerate()
    {
        let request = operations::ActionRequest {
            control_revision: service.control_revision(),
            machine: "fixture".into(),
            service: "managed-fixture".into(),
            action,
            request_id: format!("12345678-1234-1234-1234-{index:012x}"),
        };
        let result = operations::execute(root.path().into(), service.clone(), request.clone())
            .await
            .unwrap();
        assert_eq!(result.outcome, operations::Outcome::Succeeded, "{result:?}");
        let replay = operations::execute(root.path().into(), service.clone(), request)
            .await
            .unwrap();
        assert_eq!(replay.completed_at, result.completed_at);
        assert_eq!(replay.outcome, result.outcome);
    }
    assert_eq!(operations::history(root.path()).unwrap().len(), 4);
    // Leave a real restart's request receipt without its result, then retry the
    // same ID. A second restart would change MainPID.
    let started = super::manager::action(&service, model::Action::Start)
        .await
        .unwrap();
    assert!(started.success);
    let mut interrupted = service.clone();
    interrupted.interrupt_after_command = true;
    let request = operations::ActionRequest {
        machine: "fixture".into(),
        service: "managed-fixture".into(),
        action: model::Action::Restart,
        request_id: "12345678-1234-1234-1234-000000000099".into(),
        control_revision: service.control_revision(),
    };
    assert!(
        operations::execute(root.path().into(), interrupted.clone(), request.clone())
            .await
            .unwrap_err()
            .contains("Simulated interruption")
    );
    let before = super::manager::status(&service).await.unwrap().1;
    let replay = operations::execute(root.path().into(), interrupted, request)
        .await
        .unwrap();
    assert_eq!(replay.outcome, operations::Outcome::Unknown);
    assert_eq!(super::manager::status(&service).await.unwrap().1, before);
    assert!(
        super::manager::action(&service, model::Action::Stop)
            .await
            .unwrap()
            .success
    );
}
