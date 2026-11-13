//! `mash qdrant install` against a local fake release server, a simulated
//! supervisor and a disposable directory standing in for `HOME`. No test
//! reaches the network or a real service manager.
use super::release::{Asset, Format, Platform, Source, VERSION};
use super::supervisor::{self, Control, ServiceState, Step, Supervisor};
use super::{Account, Host, InstallOutcome, Layout, Ports, UninstallOutcome};
use crate::ErrorKind;
use sha2::Digest;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const EXECUTABLE: &[u8] = b"#!/bin/sh\necho fake qdrant\n";

/// How the fake release server answers.
#[derive(Clone)]
enum Serve {
    Archive(Vec<u8>),
    /// Declare the full length, send half, then close the connection.
    Truncated(Vec<u8>),
}

struct ReleaseServer {
    base: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for ReleaseServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn release_server(serve: Serve) -> ReleaseServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/download", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let counted = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            counted.fetch_add(1, Ordering::SeqCst);
            let serve = serve.clone();
            tokio::spawn(async move {
                read_request(&mut stream).await;
                let (body, declared) = match &serve {
                    Serve::Archive(bytes) => (bytes.as_slice(), bytes.len()),
                    Serve::Truncated(bytes) => (&bytes[..bytes.len() / 2], bytes.len()),
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {declared}\r\nconnection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(body).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    ReleaseServer {
        base,
        requests,
        task,
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    String::from_utf8_lossy(&request).into_owned()
}

/// A stand-in Qdrant: `/` names the version, `/readyz` answers, and the
/// gRPC port accepts connections.
async fn fake_qdrant(ports: Ports, version: &str) -> Vec<tokio::task::JoinHandle<()>> {
    let http = TcpListener::bind(("127.0.0.1", ports.http)).await.unwrap();
    let grpc = TcpListener::bind(("127.0.0.1", ports.grpc)).await.unwrap();
    let version = version.to_owned();
    vec![
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = http.accept().await else {
                    return;
                };
                let version = version.clone();
                tokio::spawn(async move {
                    let request = read_request(&mut stream).await;
                    let body = if request.starts_with("GET /readyz") {
                        "all shards are ready".to_owned()
                    } else {
                        format!(
                            "{{\"title\":\"qdrant - vector search engine\",\"version\":\"{version}\"}}"
                        )
                    };
                    let _ = stream
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .await;
                });
            }
        }),
        tokio::spawn(async move {
            while let Ok((stream, _)) = grpc.accept().await {
                drop(stream);
            }
        }),
    ]
}

async fn stop_all(tasks: Vec<tokio::task::JoinHandle<()>>) {
    for task in tasks {
        task.abort();
        let _ = task.await;
    }
}

/// A supervisor that serves a fake Qdrant on the configured ports while
/// "running" and records every step it was asked to take.
pub(crate) struct FakeSupervisor {
    ports: Ports,
    inner: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    registered: bool,
    running: Vec<tokio::task::JoinHandle<()>>,
    steps: Vec<Step>,
}

impl FakeSupervisor {
    fn new(ports: Ports) -> Arc<Self> {
        Arc::new(Self {
            ports,
            inner: Mutex::new(FakeState::default()),
        })
    }

    pub(crate) fn state(&self) -> ServiceState {
        let inner = self.inner.lock().unwrap();
        if !inner.running.is_empty() {
            ServiceState::Running
        } else if inner.registered {
            ServiceState::Stopped
        } else {
            ServiceState::NotInstalled
        }
    }

    async fn shutdown(&self) {
        let running = std::mem::take(&mut self.inner.lock().unwrap().running);
        stop_all(running).await;
    }

    fn steps(&self) -> Vec<Step> {
        self.inner.lock().unwrap().steps.clone()
    }

    pub(crate) async fn perform(&self, layout: &Layout, step: Step) -> Result<(), crate::AppError> {
        let running = {
            let mut inner = self.inner.lock().unwrap();
            inner.steps.push(step);
            std::mem::take(&mut inner.running)
        };
        stop_all(running).await;
        match step {
            Step::Start => {
                assert!(
                    layout.definition().is_file(),
                    "started before the definition was written"
                );
                let tasks = fake_qdrant(self.ports, VERSION).await;
                let mut inner = self.inner.lock().unwrap();
                inner.registered = true;
                inner.running = tasks;
            }
            Step::Remove => self.inner.lock().unwrap().registered = false,
            Step::Stop | Step::Query => {}
        }
        Ok(())
    }
}

fn free_ports() -> Ports {
    let port = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let http = port();
    let mut grpc = port();
    while grpc == http {
        grpc = port();
    }
    Ports { http, grpc }
}

fn config(ports: Ports) -> crate::config::QdrantConfig {
    crate::config::QdrantConfig {
        url: format!("http://127.0.0.1:{}", ports.http),
        grpc_url: format!("http://127.0.0.1:{}", ports.grpc),
        ..Default::default()
    }
}

fn tar_gz(name: &str, contents: &[u8]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder.append_data(&mut header, name, contents).unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

fn asset_for(bytes: &[u8], format: Format) -> Asset {
    Asset {
        name: "qdrant-test.tar.gz".into(),
        size: bytes.len() as u64,
        sha256: hex::encode(sha2::Sha256::digest(bytes)),
        format,
    }
}

fn account() -> Account {
    Account {
        user: "WORKSTATION\\ada".into(),
        system_drive: "D:".into(),
        system_root: "D:\\Windows".into(),
    }
}

fn layout(home: &Path, platform: Platform) -> Layout {
    Layout::for_platform(
        platform,
        home.join(".local/share/mash/qdrant"),
        home,
        &home.join(".config"),
        None,
        account(),
    )
}

struct Fixture {
    home: tempfile::TempDir,
    ports: Ports,
    server: ReleaseServer,
    fake: Arc<FakeSupervisor>,
    host: Host,
}

async fn fixture(serve: Serve, asset: Asset) -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let ports = free_ports();
    let server = release_server(serve).await;
    let fake = FakeSupervisor::new(ports);
    let host = Host {
        layout: layout(home.path(), Platform::LinuxX86_64),
        source: Source {
            base: server.base.clone(),
            asset,
            executable: "qdrant",
        },
        control: Control::Fake(fake.clone()),
        ready_within: std::time::Duration::from_secs(10),
    };
    Fixture {
        home,
        ports,
        server,
        fake,
        host,
    }
}

async fn installable() -> Fixture {
    let archive = tar_gz("qdrant", EXECUTABLE);
    let asset = asset_for(&archive, Format::TarGz);
    fixture(Serve::Archive(archive), asset).await
}

/// Nothing but the (empty) lock file may remain after a failed install.
fn assert_nothing_installed(fixture: &Fixture) {
    let root = &fixture.host.layout.root;
    let left: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(left, vec!["install.lock".to_owned()], "{root:?}");
    assert!(!fixture.host.layout.definition().exists());
    assert_eq!(fixture.fake.steps(), []);
}

#[test]
fn each_platform_selects_its_pinned_release_archive() {
    let expected = [
        (
            ("linux", "x86_64"),
            "qdrant-x86_64-unknown-linux-musl.tar.gz",
            Format::TarGz,
        ),
        (
            ("linux", "aarch64"),
            "qdrant-aarch64-unknown-linux-musl.tar.gz",
            Format::TarGz,
        ),
        (
            ("macos", "x86_64"),
            "qdrant-x86_64-apple-darwin.tar.gz",
            Format::TarGz,
        ),
        (
            ("macos", "aarch64"),
            "qdrant-aarch64-apple-darwin.tar.gz",
            Format::TarGz,
        ),
        (
            ("windows", "x86_64"),
            "qdrant-x86_64-pc-windows-msvc.zip",
            Format::Zip,
        ),
    ];
    for ((os, arch), name, format) in expected {
        let platform = Platform::from_target(os, arch).unwrap();
        let asset = platform.asset();
        assert_eq!((asset.name.as_str(), asset.format), (name, format));
        assert_eq!(asset.sha256.len(), 64);
        assert!(
            asset
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert!((20_000_000..50_000_000).contains(&asset.size));
        let source = Source::github(platform);
        assert_eq!(
            source.base,
            "https://github.com/qdrant/qdrant/releases/download/v1.20.0"
        );
    }
    // The Windows digest is the one CI already verifies.
    assert_eq!(
        Platform::WindowsX86_64.asset().sha256,
        "6d13553584fd0411f00637519e293331ff0aa1fb79bd9e30db45839bb75de2be"
    );
    assert_eq!(Platform::WindowsX86_64.executable(), "qdrant.exe");
    for (os, arch) in [
        ("linux", "x86"),
        ("windows", "aarch64"),
        ("freebsd", "x86_64"),
        ("linux", "riscv64"),
    ] {
        assert_eq!(Platform::from_target(os, arch), None, "{os} {arch}");
    }
    let supervisors: Vec<_> = Platform::ALL.iter().map(|p| p.supervisor()).collect();
    assert_eq!(
        supervisors,
        [
            Supervisor::SystemdUser,
            Supervisor::SystemdUser,
            Supervisor::LaunchAgent,
            Supervisor::LaunchAgent,
            Supervisor::ScheduledTask
        ]
    );
}

#[test]
fn only_a_keyless_loopback_http_endpoint_is_installable() {
    let defaults = Ports::from_config(&crate::config::QdrantConfig::default()).unwrap();
    assert_eq!(
        defaults,
        Ports {
            http: 6333,
            grpc: 6334
        }
    );
    let custom = crate::config::QdrantConfig {
        url: "http://localhost:16333".into(),
        grpc_url: "http://127.0.0.1:16334".into(),
        ..Default::default()
    };
    assert_eq!(
        Ports::from_config(&custom).unwrap(),
        Ports {
            http: 16333,
            grpc: 16334
        }
    );
    for (url, grpc_url, key) in [
        ("http://qdrant.internal:6333", "http://127.0.0.1:6334", None),
        ("https://127.0.0.1:6333", "http://127.0.0.1:6334", None),
        ("http://127.0.0.1:6333", "http://127.0.0.1:6333", None),
        (
            "http://127.0.0.1:6333",
            "http://127.0.0.1:6334",
            Some("QDRANT_KEY".to_owned()),
        ),
    ] {
        let config = crate::config::QdrantConfig {
            url: url.into(),
            grpc_url: grpc_url.into(),
            api_key_env: key,
            ..Default::default()
        };
        let error = Ports::from_config(&config).unwrap_err();
        assert_eq!(error.error.kind, ErrorKind::Configuration, "{url}");
    }
}

#[tokio::test]
async fn a_fresh_install_verifies_places_registers_and_starts_qdrant() {
    let fixture = installable().await;
    let response = super::install(&fixture.host, &config(fixture.ports), false)
        .await
        .unwrap();
    assert_eq!(response.outcome, InstallOutcome::Installed);
    assert_eq!(response.version, VERSION);
    assert_eq!(response.downloaded_bytes, fixture.host.source.asset.size);
    assert_eq!(response.recommended_action, crate::Action::Init);
    assert_eq!(
        response.url,
        format!("http://127.0.0.1:{}", fixture.ports.http)
    );
    let layout = &fixture.host.layout;
    let binary = layout.binary(VERSION);
    assert_eq!(std::fs::read(&binary).unwrap(), EXECUTABLE);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The file is created 0o755 under the caller's umask, so only the
        // owner's execute bit is certain (release gates run with 0o077).
        assert_eq!(
            std::fs::metadata(&binary).unwrap().permissions().mode() & 0o100,
            0o100
        );
    }
    assert!(layout.storage().is_dir() && layout.snapshots().is_dir());
    let unit = std::fs::read_to_string(layout.definition()).unwrap();
    assert!(unit.contains(&format!("ExecStart=\"{}\" --config-path", binary.display())));
    let qdrant = std::fs::read_to_string(layout.qdrant_config()).unwrap();
    assert!(qdrant.contains("host: 127.0.0.1"));
    assert!(qdrant.contains(&format!("http_port: {}", fixture.ports.http)));
    assert!(qdrant.contains("telemetry_disabled: true"));
    assert_eq!(fixture.fake.steps(), [Step::Start]);
    assert_eq!(fixture.server.requests.load(Ordering::SeqCst), 1);
    let status = super::report_on(&fixture.host).await.unwrap();
    assert_eq!(status.state, ServiceState::Running);
    assert_eq!(status.version, VERSION);
    assert_eq!(status.service, "mash-qdrant.service");
    let staging = std::fs::read_dir(&layout.root)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".download-")
        })
        .count();
    assert_eq!(staging, 0);
    fixture.fake.shutdown().await;
}

#[tokio::test]
async fn rerunning_changes_nothing_then_repairs_without_touching_storage() {
    let fixture = installable().await;
    let config = config(fixture.ports);
    super::install(&fixture.host, &config, true).await.unwrap();
    let layout = &fixture.host.layout;
    let kept = layout.storage().join("collections/kept.txt");
    std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
    std::fs::write(&kept, "search views").unwrap();

    let modified = |path: &Path| std::fs::metadata(path).unwrap().modified().unwrap();
    let written = modified(&layout.definition());
    let again = super::install(&fixture.host, &config, true).await.unwrap();
    assert_eq!(again.outcome, InstallOutcome::Current);
    assert_eq!(
        modified(&layout.definition()),
        written,
        "an unchanged unit is not rewritten"
    );
    assert_eq!(again.downloaded_bytes, 0);
    assert_eq!(again.recommended_action, crate::Action::Index);
    assert_eq!(
        fixture.fake.steps(),
        [Step::Start],
        "a current service is left running"
    );
    assert_eq!(fixture.server.requests.load(Ordering::SeqCst), 1);

    // A deleted unit is rewritten and the service restarted around it.
    std::fs::remove_file(layout.definition()).unwrap();
    let repaired = super::install(&fixture.host, &config, true).await.unwrap();
    assert_eq!(repaired.outcome, InstallOutcome::Repaired);
    assert!(layout.definition().is_file());
    assert_eq!(fixture.fake.steps(), [Step::Start, Step::Stop, Step::Start]);

    // A stopped service is started again.
    fixture.fake.perform(layout, Step::Stop).await.unwrap();
    let restarted = super::install(&fixture.host, &config, true).await.unwrap();
    assert_eq!(restarted.outcome, InstallOutcome::Repaired);
    assert_eq!(
        super::report_on(&fixture.host).await.unwrap().state,
        ServiceState::Running
    );

    // A missing binary is downloaded again.
    std::fs::remove_dir_all(layout.root.join("bin")).unwrap();
    let restored = super::install(&fixture.host, &config, true).await.unwrap();
    assert_eq!(restored.outcome, InstallOutcome::Repaired);
    assert_eq!(fixture.server.requests.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), "search views");
    fixture.fake.shutdown().await;
}

#[tokio::test]
async fn an_earlier_version_is_stopped_replaced_and_removed() {
    let fixture = installable().await;
    let config = config(fixture.ports);
    super::install(&fixture.host, &config, true).await.unwrap();
    let layout = &fixture.host.layout;
    // Pretend the running service came from an earlier pinned release.
    let manifest_path = layout.root.join(super::MANIFEST);
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    std::fs::write(&manifest_path, manifest.replace(VERSION, "1.19.0")).unwrap();
    std::fs::rename(
        layout.root.join("bin").join(VERSION),
        layout.root.join("bin/1.19.0"),
    )
    .unwrap();

    let upgraded = super::install(&fixture.host, &config, true).await.unwrap();
    assert_eq!(upgraded.outcome, InstallOutcome::Upgraded);
    assert_eq!(fixture.fake.steps(), [Step::Start, Step::Stop, Step::Start]);
    assert!(layout.binary(VERSION).is_file());
    assert!(!layout.root.join("bin/1.19.0").exists());
    let unit = std::fs::read_to_string(layout.definition()).unwrap();
    assert!(unit.contains(&format!("/bin/{VERSION}/qdrant")));
    fixture.fake.shutdown().await;
}

#[tokio::test]
async fn a_checksum_mismatch_installs_nothing() {
    let archive = tar_gz("qdrant", EXECUTABLE);
    let mut asset = asset_for(&archive, Format::TarGz);
    asset.sha256 = "0".repeat(64);
    let fixture = fixture(Serve::Archive(archive), asset).await;
    let error = super::install(&fixture.host, &config(fixture.ports), false)
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::QdrantInstall);
    assert!(
        error.error.message.contains("does not match the pinned"),
        "{}",
        error.error.message
    );
    assert_nothing_installed(&fixture);
}

#[tokio::test]
async fn an_interrupted_download_installs_nothing_and_can_be_retried() {
    let archive = tar_gz("qdrant", EXECUTABLE);
    let asset = asset_for(&archive, Format::TarGz);
    let fixture = fixture(Serve::Truncated(archive), asset).await;
    let error = super::install(&fixture.host, &config(fixture.ports), false)
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::QdrantInstall);
    assert!(error.error.retryable, "{}", error.error.message);
    assert_nothing_installed(&fixture);
}

#[tokio::test]
async fn a_killed_download_leaves_staging_that_the_next_run_removes() {
    let fixture = installable().await;
    let stale = fixture.host.layout.root.join(".download-killed");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("qdrant-test.tar.gz"), b"partial").unwrap();
    super::install(&fixture.host, &config(fixture.ports), false)
        .await
        .unwrap();
    assert!(!stale.exists());
    fixture.fake.shutdown().await;
}

#[tokio::test]
async fn an_archive_without_qdrant_installs_nothing() {
    let archive = tar_gz("README.md", b"not a server");
    let asset = asset_for(&archive, Format::TarGz);
    let fixture = fixture(Serve::Archive(archive), asset).await;
    let error = super::install(&fixture.host, &config(fixture.ports), false)
        .await
        .unwrap_err();
    assert!(
        error.error.message.contains("holds no qdrant"),
        "{}",
        error.error.message
    );
    assert_nothing_installed(&fixture);
}

#[tokio::test]
async fn ports_held_by_another_program_are_refused_before_downloading() {
    let fixture = installable().await;
    // Something that answers HTTP, but is not Qdrant.
    let other = TcpListener::bind(("127.0.0.1", fixture.ports.http))
        .await
        .unwrap();
    let web = tokio::spawn(async move {
        while let Ok((mut stream, _)) = other.accept().await {
            read_request(&mut stream).await;
            let _ = stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nhello",
                )
                .await;
        }
    });
    let error = super::install(&fixture.host, &config(fixture.ports), false)
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::Conflict);
    assert!(
        error.error.message.contains("not Qdrant"),
        "{}",
        error.error.message
    );
    assert_eq!(fixture.server.requests.load(Ordering::SeqCst), 0);
    assert_nothing_installed(&fixture);
    stop_all(vec![web]).await;

    // Only the gRPC port taken.
    let grpc = TcpListener::bind(("127.0.0.1", fixture.ports.grpc))
        .await
        .unwrap();
    let error = super::install(&fixture.host, &config(fixture.ports), false)
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::Conflict);
    assert!(
        error.error.message.contains("gRPC port"),
        "{}",
        error.error.message
    );
    drop(grpc);
    assert_nothing_installed(&fixture);
}

#[tokio::test]
async fn a_compatible_qdrant_already_running_is_reported_and_left_alone() {
    let fixture = installable().await;
    let running = fake_qdrant(fixture.ports, "1.20.3").await;
    let response = super::install(&fixture.host, &config(fixture.ports), true)
        .await
        .unwrap();
    assert_eq!(response.outcome, InstallOutcome::AlreadyRunning);
    assert_eq!(response.version, "1.20.3");
    assert!(response.service.is_none());
    assert_eq!(fixture.server.requests.load(Ordering::SeqCst), 0);
    assert_nothing_installed(&fixture);
    stop_all(running).await;

    // An older one is not compatible.
    let old = fake_qdrant(fixture.ports, "1.19.2").await;
    let error = super::install(&fixture.host, &config(fixture.ports), true)
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::Conflict);
    assert!(
        error.error.message.contains("Qdrant 1.19.2"),
        "{}",
        error.error.message
    );
    stop_all(old).await;
    assert!(super::probe::is_compatible("1.20.0") && super::probe::is_compatible("1.31.1"));
    assert!(!super::probe::is_compatible("2.0.0") && !super::probe::is_compatible("1.9"));
}

#[tokio::test]
async fn uninstall_removes_the_service_and_keeps_storage_unless_asked() {
    let fixture = installable().await;
    super::install(&fixture.host, &config(fixture.ports), true)
        .await
        .unwrap();
    let layout = &fixture.host.layout;
    std::fs::write(layout.storage().join("kept"), "data").unwrap();
    let removed = super::uninstall(&fixture.host, false).await.unwrap();
    assert_eq!(removed.outcome, UninstallOutcome::Removed);
    assert_eq!(removed.service.as_deref(), Some("mash-qdrant.service"));
    assert_eq!(removed.kept_storage, Some(layout.storage()));
    assert_eq!(fixture.fake.steps(), [Step::Start, Step::Remove]);
    assert!(!layout.definition().exists() && !layout.root.join("bin").exists());
    assert!(!layout.root.join(super::MANIFEST).exists());
    assert!(layout.storage().join("kept").is_file());
    assert!(super::report_on(&fixture.host).await.is_none());

    let deleted = super::uninstall(&fixture.host, true).await.unwrap();
    assert!(deleted.removed.contains(&layout.storage()));
    assert!(!layout.storage().exists() && deleted.kept_storage.is_none());
    assert!(
        !layout.root.exists(),
        "an emptied installation folder is removed"
    );
    let home = fixture.home.path().join("nothing-here");
    let empty = Host {
        layout: super::Layout::for_platform(
            Platform::LinuxX86_64,
            home.join("qdrant"),
            &home,
            &home,
            None,
            account(),
        ),
        source: Source::github(Platform::LinuxX86_64),
        control: Control::Fake(FakeSupervisor::new(fixture.ports)),
        ready_within: std::time::Duration::from_secs(1),
    };
    assert_eq!(
        super::uninstall(&empty, false).await.unwrap().outcome,
        UninstallOutcome::NotInstalled
    );
    assert!(!home.exists());
}

#[test]
fn a_windows_zip_unpacks_its_executable() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("qdrant.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    zip.start_file("qdrant.exe", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"MZ fake").unwrap();
    zip.finish().unwrap();
    let target = directory.path().join("qdrant.exe");
    super::release::unpack(&archive, Format::Zip, "qdrant.exe", &target).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"MZ fake");
}

#[test]
fn the_systemd_unit_runs_the_pinned_binary_as_a_user_service() {
    let home = Path::new("/home/ada");
    let layout = layout(home, Platform::LinuxAarch64);
    assert_eq!(
        layout.definition(),
        home.join(".config/systemd/user/mash-qdrant.service")
    );
    let unit = supervisor::definition(&layout, &layout.binary(VERSION)).unwrap();
    let root = "/home/ada/.local/share/mash/qdrant";
    for line in [
        "[Service]".to_owned(),
        format!("WorkingDirectory={root}"),
        format!("ExecStart=\"{root}/bin/1.20.0/qdrant\" --config-path \"{root}/config.yaml\""),
        "Restart=on-failure".to_owned(),
        "WantedBy=default.target".to_owned(),
    ] {
        assert!(
            unit.lines().any(|candidate| candidate == line),
            "{line}\n{unit}"
        );
    }
    assert_eq!(
        supervisor::commands(&layout, Step::Start, "")
            .iter()
            .map(|command| format!("{} {}", command.program, command.args.join(" ")))
            .collect::<Vec<_>>(),
        [
            "systemctl --user daemon-reload",
            "systemctl --user enable mash-qdrant.service",
            "systemctl --user restart mash-qdrant.service",
        ]
    );
    // `%` would otherwise start a systemd specifier.
    let odd = Layout::for_platform(
        Platform::LinuxX86_64,
        "/home/a%b/qdrant".into(),
        home,
        &home.join(".config"),
        Some("mash-qdrant-e2e".into()),
        account(),
    );
    assert_eq!(odd.service, "mash-qdrant-e2e.service");
    let unit = supervisor::definition(&odd, &odd.binary(VERSION)).unwrap();
    assert!(
        unit.contains("ExecStart=\"/home/a%%b/qdrant/bin/1.20.0/qdrant\""),
        "{unit}"
    );
    let quoted = Layout::for_platform(
        Platform::LinuxX86_64,
        "/home/a\"b/qdrant".into(),
        home,
        home,
        None,
        account(),
    );
    assert!(supervisor::definition(&quoted, &quoted.binary(VERSION)).is_err());
}

#[test]
fn the_launch_agent_starts_at_login_and_keeps_qdrant_running() {
    let home = Path::new("/Users/ada & co");
    let layout = layout(home, Platform::MacosAarch64);
    assert_eq!(
        layout.definition(),
        home.join("Library/LaunchAgents/dev.mash.qdrant.plist")
    );
    let plist = supervisor::definition(&layout, &layout.binary(VERSION)).unwrap();
    let root = "/Users/ada &amp; co/.local/share/mash/qdrant";
    for part in [
        "<key>Label</key><string>dev.mash.qdrant</string>".to_owned(),
        format!(
            "<key>ProgramArguments</key><array><string>{root}/bin/1.20.0/qdrant</string><string>--config-path</string><string>{root}/config.yaml</string></array>"
        ),
        "<key>RunAtLoad</key><true/>".to_owned(),
        "<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>".to_owned(),
        format!("<key>StandardOutPath</key><string>{root}/qdrant.log</string>"),
    ] {
        assert!(plist.contains(&part), "{part}\n{plist}");
    }
    let commands: Vec<_> = supervisor::commands(&layout, Step::Start, "gui/501")
        .iter()
        .map(|command| format!("{} {}", command.program, command.args.join(" ")))
        .collect();
    assert_eq!(
        commands,
        [
            "launchctl bootout gui/501/dev.mash.qdrant".to_owned(),
            "launchctl enable gui/501/dev.mash.qdrant".to_owned(),
            format!(
                "launchctl bootstrap gui/501 {}",
                home.join("Library/LaunchAgents/dev.mash.qdrant.plist")
                    .display()
            ),
            "launchctl kickstart gui/501/dev.mash.qdrant".to_owned(),
        ]
    );
}

#[test]
fn the_windows_logon_task_needs_no_administrator_and_opens_no_window() {
    let home = Path::new("C:\\Users\\ada");
    let layout = Layout::for_platform(
        Platform::WindowsX86_64,
        "C:\\Users\\ada\\AppData\\Local\\kojah\\mash\\data\\qdrant".into(),
        home,
        home,
        None,
        account(),
    );
    // Qdrant's data goes to a short folder on the system drive.
    assert_eq!(
        layout.storage(),
        Path::new("D:\\mash-qdrant").join("storage")
    );
    let task = supervisor::definition(&layout, &layout.binary(VERSION)).unwrap();
    for part in [
        "<LogonTrigger><Enabled>true</Enabled><UserId>WORKSTATION\\ada</UserId></LogonTrigger>",
        "<LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel>",
        "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
        "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
        "<Command>D:\\Windows\\System32\\conhost.exe</Command>",
        "<Arguments>--headless &quot;",
    ] {
        assert!(task.contains(part), "{part}\n{task}");
    }
    let config = supervisor::qdrant_config(
        &layout,
        Ports {
            http: 6333,
            grpc: 6334,
        },
    );
    assert!(
        config.contains("storage_path: 'D:\\mash-qdrant"),
        "{config}"
    );
    assert!(config.contains("on_disk:"), "{config}");
    let start = supervisor::commands(&layout, Step::Start, "");
    assert!(
        start
            .iter()
            .all(|command| command.program == "powershell.exe")
    );
    assert!(
        start[1]
            .args
            .last()
            .unwrap()
            .starts_with("Register-ScheduledTask -TaskName 'MashQdrant' -Xml")
    );
    assert_eq!(
        start[2].args.last().unwrap(),
        "Start-ScheduledTask -TaskName 'MashQdrant'"
    );
    let mut nobody = layout.clone();
    nobody.account.user.clear();
    assert!(supervisor::definition(&nobody, &nobody.binary(VERSION)).is_err());
}

#[test]
fn supervisor_reports_map_to_service_states() {
    use supervisor::parse_state;
    let systemd = |output: &str| parse_state(Supervisor::SystemdUser, true, output);
    assert_eq!(
        systemd("LoadState=loaded\nActiveState=active\n"),
        ServiceState::Running
    );
    assert_eq!(
        systemd("LoadState=loaded\nActiveState=activating\n"),
        ServiceState::Starting
    );
    assert_eq!(
        systemd("LoadState=loaded\nActiveState=failed\n"),
        ServiceState::Failed
    );
    assert_eq!(
        systemd("LoadState=loaded\nActiveState=inactive\n"),
        ServiceState::Stopped
    );
    assert_eq!(
        systemd("LoadState=not-found\nActiveState=inactive\n"),
        ServiceState::NotInstalled
    );
    assert_eq!(
        parse_state(Supervisor::SystemdUser, false, ""),
        ServiceState::Unknown
    );
    let launchd = "gui/501/dev.mash.qdrant = {\n\tactive count = 1\n\tstate = running\n";
    assert_eq!(
        parse_state(Supervisor::LaunchAgent, true, launchd),
        ServiceState::Running
    );
    assert_eq!(
        parse_state(Supervisor::LaunchAgent, true, "\tstate = not running\n"),
        ServiceState::Stopped
    );
    assert_eq!(
        parse_state(Supervisor::LaunchAgent, false, "Could not find service"),
        ServiceState::NotInstalled
    );
    for (output, state) in [
        ("Running\r\n", ServiceState::Running),
        ("Ready\r\n", ServiceState::Stopped),
        ("Missing\r\n", ServiceState::NotInstalled),
        ("Queued", ServiceState::Starting),
    ] {
        assert_eq!(parse_state(Supervisor::ScheduledTask, true, output), state);
    }
}
