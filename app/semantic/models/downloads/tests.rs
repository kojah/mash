use super::super::{Backend, Models, assets, generations::StagedAssets, test_support};
use super::*;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn installation_child() {
    let Some(root) = std::env::var_os("MASH_DOWNLOAD_TEST_ROOT") else {
        return;
    };
    let models = Models::new(root.into());
    if std::env::var("MASH_DOWNLOAD_TEST_FAILURE").is_ok() {
        let before = fs::read(models.root.join(super::super::MARKER_NAME)).unwrap();
        let mut backend = models.load().unwrap().unwrap();
        let expected = backend
            .embed(&["preserve the working installation"])
            .unwrap();
        let stage = StagedAssets::new(&models.root).unwrap();
        let info = TextEmbedding::get_model_info(&EMBEDDING_MODEL).unwrap();
        let snapshot = assets::snapshot(stage.root(), super::super::EMBEDDING_REPOSITORY).unwrap();
        fs::remove_file(snapshot.join(&info.model_file)).unwrap();
        let Err(error) = Backend::download_and_load(stage.root()) else {
            panic!("truncated SDK download unexpectedly succeeded");
        };
        assert_eq!(error.error.kind, crate::ErrorKind::Model);
        assert!(
            error
                .error
                .message
                .contains("failed to download model asset")
        );
        assert!(
            walkdir::WalkDir::new(stage.root())
                .into_iter()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "part"))
        );
        let path = stage.root().to_path_buf();
        drop(stage);
        assert!(!path.exists());
        assert_eq!(
            fs::read(models.root.join(super::super::MARKER_NAME)).unwrap(),
            before
        );
        assert_eq!(
            backend
                .embed(&["preserve the working installation"])
                .unwrap(),
            expected
        );
        let mut reopened = models.load().unwrap().unwrap();
        assert_eq!(
            reopened
                .embed(&["preserve the working installation"])
                .unwrap(),
            expected
        );
    } else {
        models.install().unwrap();
        models.require_installed().unwrap();
        let mut backend = models.load().unwrap().unwrap();
        assert_eq!(
            backend.embed(&["model cache isolation"]).unwrap()[0].len(),
            384
        );
    }
}

fn installer(root: &Path, external: &Path, endpoint: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "semantic::models::downloads::tests::installation_child",
            "--nocapture",
        ])
        .env("MASH_DOWNLOAD_TEST_ROOT", root)
        .env("HF_HOME", external)
        .env("HF_ENDPOINT", endpoint);
    command
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn explicit_cache_wins_over_hf_home_without_changing_process_environment() {
    let root = test_support::copied_models();
    let external = tempfile::tempdir().unwrap();
    let output = installer(root.path(), external.path(), "http://127.0.0.1:1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_dir(external.path()).unwrap().count(), 0);
    assert!(Models::new(root.path().to_path_buf()).is_installed());
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn truncated_sdk_download_preserves_existing_models_and_removes_partial_stage() {
    let root = test_support::copied_models();
    let external = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let mut child = installer(root.path(), external.path(), &endpoint)
        .env("MASH_DOWNLOAD_TEST_FAILURE", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // A real HF Hub metadata request succeeds; the body download then ends
    // before its advertised Content-Length. No public model endpoint is used.
    let requests = serve_truncated_download(&listener);
    if requests.is_err() {
        let _ = child.kill();
    }
    let output = child.wait_with_output().unwrap();
    requests.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_dir(external.path()).unwrap().count(), 0);
}

fn serve_truncated_download(listener: &TcpListener) -> std::io::Result<()> {
    for metadata in [true, false] {
        let deadline = Instant::now() + crate::test_support::HANG_GUARD;
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        };
        // macOS accepted sockets inherit the listener's nonblocking mode.
        // Wait for complete request headers using the bounded read timeout.
        socket.set_nonblocking(false)?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") && request.len() < 16 * 1024 {
            let mut byte = [0];
            if socket.read(&mut byte)? == 0 {
                break;
            }
            request.push(byte[0]);
        }
        let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
        let range = if metadata {
            "range: bytes=0-0\r\n"
        } else {
            "range: bytes=0-\r\n"
        };
        if !request.contains(range) {
            return Err(std::io::Error::other("unexpected SDK download range"));
        }
        if metadata {
            socket.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\nContent-Range: bytes 0-0/8192\r\nETag: broken-blob\r\nx-repo-commit: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nConnection: close\r\n\r\nx")?;
        } else {
            socket.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 8192\r\nConnection: close\r\n\r\npartial",
            )?;
        }
    }
    Ok(())
}

#[test]
fn unreachable_hosts_are_told_apart_from_transient_failures() {
    assert!(unreachable(
        "request error: io: failed to lookup address information: Temporary failure in name resolution"
    ));
    assert!(unreachable(
        "request error: io: Connection refused (os error 111)"
    ));
    assert!(!unreachable("request error: http status: 503"));
    assert!(!unreachable(
        "Too many retries: I/O error unexpected end of file"
    ));
}
