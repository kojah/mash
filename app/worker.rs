use std::fs::{self, OpenOptions};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::AppError;
use crate::cli::{Cli, SearchResponse};

const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REQUEST: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 64 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
struct Reply {
    code: i32,
    result: Result<SearchResponse, AppError>,
}

fn uid() -> std::io::Result<u32> {
    let (stream, _peer) = UnixStream::pair()?;
    Ok(stream.peer_cred()?.uid())
}

fn private_directory(path: &Path, owner: u32) -> std::io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != owner || metadata.mode() & 0o777 != 0o700 {
        return Err(std::io::Error::other(
            "search worker directory is not private",
        ));
    }
    Ok(())
}

fn socket_path(profile: &[u8], environment: &[String], owner: u32) -> std::io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let metadata = fs::metadata(&executable)?;
    let directory = PathBuf::from(format!("/tmp/mash-search-{owner}"));
    private_directory(&directory, owner)?;
    let mut hash = blake3::Hasher::new();
    hash.update(executable.as_os_str().as_bytes());
    hash.update(&metadata.len().to_le_bytes());
    hash.update(&metadata.mtime().to_le_bytes());
    hash.update(&metadata.mtime_nsec().to_le_bytes());
    hash.update(profile);
    let mut names = environment.to_vec();
    names.sort();
    names.dedup();
    for name in names {
        hash.update(name.as_bytes());
        hash.update(&[0]);
        if let Some(value) = std::env::var_os(&name) {
            hash.update(&[1]);
            hash.update(value.as_os_str().as_bytes());
        }
        hash.update(&[0]);
    }
    Ok(directory.join(format!("{}.sock", &hash.finalize().to_hex()[..24])))
}

async fn connect(path: &Path, owner: u32) -> std::io::Result<UnixStream> {
    let stream = tokio::time::timeout(Duration::from_secs(1), UnixStream::connect(path))
        .await
        .map_err(|_| std::io::Error::other("search worker connection timeout"))??;
    if stream.peer_cred()?.uid() != owner {
        return Err(std::io::Error::other("search worker identity mismatch"));
    }
    Ok(stream)
}

async fn start_or_connect(path: &Path, owner: u32) -> std::io::Result<UnixStream> {
    if let Ok(stream) = connect(path, owner).await {
        return Ok(stream);
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path.with_file_name("startup.lock"))?;
    let mut started = false;
    // The kernel lock is released if a starting CLI crashes. It also prevents
    // concurrent callers from removing a newly bound socket as "stale".
    for _ in 0..250 {
        if let Ok(stream) = connect(path, owner).await {
            return Ok(stream);
        }
        if !started && lock.try_lock().is_ok() {
            if let Ok(stream) = connect(path, owner).await {
                return Ok(stream);
            }
            match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_socket() && metadata.uid() == owner => {
                    fs::remove_file(path)?;
                }
                Ok(_) => return Err(std::io::Error::other("invalid search worker socket")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let child = Command::new(std::env::current_exe()?)
                .arg("--internal-search-worker")
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()?;
            // The CLI exits before its idle worker; the worker is then reaped
            // by the OS rather than keeping the CLI alive for its lifetime.
            drop(child);
            started = true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(std::io::Error::other("search worker did not start"))
}

async fn write_frame(stream: &mut UnixStream, bytes: &[u8], limit: usize) -> std::io::Result<()> {
    if bytes.len() > limit {
        return Err(std::io::Error::other("search worker message is too large"));
    }
    stream
        .write_u32(u32::try_from(bytes.len()).map_err(std::io::Error::other)?)
        .await?;
    stream.write_all(bytes).await
}

async fn read_frame(stream: &mut UnixStream, limit: usize) -> std::io::Result<Vec<u8>> {
    let length = stream.read_u32().await? as usize;
    if length > limit {
        return Err(std::io::Error::other("search worker message is too large"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}

pub(crate) async fn search(
    cli: &Cli,
    profile: &[u8],
    environment: &[String],
) -> Option<Result<SearchResponse, AppError>> {
    let owner = uid().ok()?;
    let path = socket_path(profile, environment, owner).ok()?;
    let request = serde_json::to_vec(cli).ok()?;
    // Retry once if an idle worker exited just as the client connected. Searches
    // are read-only; an interrupted connection can safely be retried.
    for _ in 0..2 {
        let attempt = async {
            let mut stream = start_or_connect(&path, owner).await?;
            write_frame(&mut stream, &request, MAX_REQUEST).await?;
            let bytes = read_frame(&mut stream, MAX_RESPONSE).await?;
            serde_json::from_slice::<Reply>(&bytes).map_err(std::io::Error::other)
        };
        // Let queued searches retain the operation's own HTTP timeouts instead
        // of counting time spent waiting for another search against them.
        if let Ok(reply) = attempt.await {
            return Some(reply.result.map_err(|mut error| {
                error.code = reply.code;
                error
            }));
        }
    }
    None
}

struct SocketCleanup {
    path: PathBuf,
    inode: u64,
}

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|m| m.ino() == self.inode) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

async fn serve(path: &Path, idle: Duration) -> Result<(), AppError> {
    let owner = uid().map_err(AppError::io)?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::usage("invalid worker socket path"))?;
    private_directory(parent, owner).map_err(AppError::io)?;
    let listener = UnixListener::bind(path).map_err(AppError::io)?;
    let _cleanup = SocketCleanup {
        path: path.to_path_buf(),
        inode: fs::symlink_metadata(path).map_err(AppError::io)?.ino(),
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(AppError::io)?;
    loop {
        let Ok(connection) = tokio::time::timeout(idle, listener.accept()).await else {
            return Ok(());
        };
        let (mut stream, _) = connection.map_err(AppError::io)?;
        if !stream.peer_cred().is_ok_and(|peer| peer.uid() == owner) {
            continue;
        }
        let Ok(Ok(bytes)) =
            tokio::time::timeout(Duration::from_secs(2), read_frame(&mut stream, MAX_REQUEST))
                .await
        else {
            continue;
        };
        let result = match serde_json::from_slice::<Cli>(&bytes) {
            Ok(cli) => crate::cli::worker_search(cli).await,
            Err(_) => Err(AppError::usage("invalid search worker request")),
        };
        let code = result.as_ref().err().map_or(0, |error| error.code);
        let reply = serde_json::to_vec(&Reply { code, result })
            .map_err(|_| AppError::internal("serialize search worker response"))?;
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            write_frame(&mut stream, &reply, MAX_RESPONSE),
        )
        .await;
    }
}

pub(crate) fn run(path: &Path) -> Result<(), AppError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| AppError::internal("start search worker runtime"))?
        .block_on(serve(path, IDLE_TIMEOUT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn worker_rejects_mutations_and_exits_after_idle_without_leaving_socket() {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("test.sock");
        let socket = path.clone();
        let task = tokio::spawn(async move { serve(&socket, Duration::from_millis(150)).await });
        let owner = uid().unwrap();
        let mut stream = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(stream) = connect(&path, owner).await {
                    break stream;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        let request = serde_json::json!({"config":null,"db":null,"models_dir":null,"command":{"Forget":{"id":"test"}}});
        write_frame(
            &mut stream,
            &serde_json::to_vec(&request).unwrap(),
            MAX_REQUEST,
        )
        .await
        .unwrap();
        let reply: Reply =
            serde_json::from_slice(&read_frame(&mut stream, MAX_RESPONSE).await.unwrap()).unwrap();
        assert_eq!(reply.code, 2);
        assert_eq!(
            reply.result.unwrap_err().error.kind,
            crate::ErrorKind::Usage
        );
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn frames_reject_oversized_inputs_before_allocating() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        writer.write_u32(1024).await.unwrap();
        assert!(read_frame(&mut reader, 10).await.is_err());
    }

    #[tokio::test]
    async fn private_directory_rejects_symlinks_and_broad_permissions() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("worker");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        let owner = uid().unwrap();
        assert!(private_directory(&directory, owner).is_err());
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        private_directory(&directory, owner).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&directory, &link).unwrap();
        assert!(private_directory(&link, owner).is_err());
    }
}
