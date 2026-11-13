//! The pinned Qdrant release: one archive per platform, its size and SHA-256
//! taken from Qdrant's GitHub release, a bounded download and extraction.
use super::{STAGING_PREFIX, Supervisor, install_error};
use crate::AppError;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

/// The Qdrant version `mash qdrant install` provides; Mash's own client and
/// CI use the same release. To move to another release, take each asset's
/// size and digest from `gh api repos/qdrant/qdrant/releases/tags/vVERSION`.
pub(crate) const VERSION: &str = "1.20.0";

/// No pinned archive is larger than this once unpacked.
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Platform {
    LinuxX86_64,
    LinuxAarch64,
    MacosX86_64,
    MacosAarch64,
    WindowsX86_64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    TarGz,
    Zip,
}

/// One downloadable archive and what it must hash to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Asset {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
    pub(crate) format: Format,
}

impl Platform {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 5] = [
        Self::LinuxX86_64,
        Self::LinuxAarch64,
        Self::MacosX86_64,
        Self::MacosAarch64,
        Self::WindowsX86_64,
    ];

    pub(crate) fn host() -> Option<Self> {
        Self::from_target(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub(crate) fn from_target(os: &str, arch: &str) -> Option<Self> {
        match (os, arch) {
            ("linux", "x86_64") => Some(Self::LinuxX86_64),
            ("linux", "aarch64") => Some(Self::LinuxAarch64),
            ("macos", "x86_64") => Some(Self::MacosX86_64),
            ("macos", "aarch64") => Some(Self::MacosAarch64),
            ("windows", "x86_64") => Some(Self::WindowsX86_64),
            _ => None,
        }
    }

    pub(crate) fn supervisor(self) -> Supervisor {
        match self {
            Self::LinuxX86_64 | Self::LinuxAarch64 => Supervisor::SystemdUser,
            Self::MacosX86_64 | Self::MacosAarch64 => Supervisor::LaunchAgent,
            Self::WindowsX86_64 => Supervisor::ScheduledTask,
        }
    }

    pub(crate) fn executable(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "qdrant.exe",
            _ => "qdrant",
        }
    }

    /// The archive from Qdrant's v1.20.0 GitHub release, with the size and
    /// digest its release API reports. Linux uses the statically linked
    /// musl builds: the `x86_64` glibc build needs glibc 2.39, and aarch64 has
    /// only a musl build. Both keep Qdrant's jemalloc allocator.
    pub(crate) fn asset(self) -> Asset {
        let (name, size, sha256) = match self {
            Self::LinuxX86_64 => (
                "qdrant-x86_64-unknown-linux-musl.tar.gz",
                33_773_335,
                "b03a64616a1f41ddab78b480a318466541765f19835318113b1821cbea8df52c",
            ),
            Self::LinuxAarch64 => (
                "qdrant-aarch64-unknown-linux-musl.tar.gz",
                31_475_234,
                "bd8a458b8ef8006ce2c603c5bd6d1d973e39d787f794008dd959112e747c3bb7",
            ),
            Self::MacosX86_64 => (
                "qdrant-x86_64-apple-darwin.tar.gz",
                31_917_921,
                "94a2e6f6b8ef3a3b5756a11172fd66554ac753e0a71c1d8d8b5349356c084f4d",
            ),
            Self::MacosAarch64 => (
                "qdrant-aarch64-apple-darwin.tar.gz",
                29_070_255,
                "531904e822050790bb8c1da6705f801ee48b91eb4c229d0d605ff06f6331b20a",
            ),
            Self::WindowsX86_64 => (
                "qdrant-x86_64-pc-windows-msvc.zip",
                30_801_664,
                "6d13553584fd0411f00637519e293331ff0aa1fb79bd9e30db45839bb75de2be",
            ),
        };
        Asset {
            name: name.to_owned(),
            size,
            sha256: sha256.to_owned(),
            format: if self == Self::WindowsX86_64 {
                Format::Zip
            } else {
                Format::TarGz
            },
        }
    }
}

/// Where the archive is downloaded from.
pub(crate) struct Source {
    /// The release's download directory, without a trailing slash.
    pub(crate) base: String,
    pub(crate) asset: Asset,
    pub(crate) executable: &'static str,
}

/// A verified, unpacked executable waiting to be moved into place. Dropping
/// it removes the staging folder, so a failed run leaves nothing behind.
pub(crate) struct Staged {
    directory: tempfile::TempDir,
    pub(crate) bytes: u64,
    executable: &'static str,
}

impl Source {
    pub(crate) fn github(platform: Platform) -> Self {
        Self {
            base: format!("https://github.com/qdrant/qdrant/releases/download/v{VERSION}"),
            asset: platform.asset(),
            executable: platform.executable(),
        }
    }

    pub(crate) fn asset_name(&self) -> &str {
        &self.asset.name
    }

    fn url(&self) -> String {
        format!("{}/{}", self.base, self.asset.name)
    }

    /// Download the archive into a staging folder under `root`, verify its
    /// size and SHA-256, and unpack the executable beside it.
    pub(crate) async fn fetch(&self, root: &Path) -> Result<Staged, AppError> {
        let directory = tempfile::Builder::new()
            .prefix(STAGING_PREFIX)
            .tempdir_in(root)
            .map_err(AppError::io)?;
        let archive = directory.path().join(&self.asset.name);
        let bytes = self.download(&archive).await?;
        let format = self.asset.format;
        let executable = self.executable;
        let target = directory.path().join(executable);
        crate::blocking(move || unpack(&archive, format, executable, &target)).await?;
        Ok(Staged {
            directory,
            bytes,
            executable,
        })
    }

    async fn download(&self, path: &Path) -> Result<u64, AppError> {
        use tokio::io::AsyncWriteExt;
        let url = self.url();
        let unreachable = |error: &dyn std::fmt::Display| {
            AppError::new(
                14,
                crate::ErrorKind::QdrantInstall,
                format!(
                    "could not download Qdrant {VERSION} from {url}: {error}; check network access to GitHub and rerun `mash qdrant install`. Nothing was installed"
                ),
                true,
            )
        };
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .read_timeout(std::time::Duration::from_secs(60))
            .user_agent(concat!("mash/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| AppError::internal(format!("HTTP client: {error}")))?;
        let mut response = client
            .get(&url)
            .send()
            .await
            .map_err(|error| unreachable(&error))?;
        if !response.status().is_success() {
            return Err(unreachable(&format!("HTTP {}", response.status())));
        }
        let expected = self.asset.size;
        if let Some(length) = response.content_length()
            && length != expected
        {
            return Err(self.mismatch(&format!("{length} bytes instead of {expected}")));
        }
        let mut file = tokio::fs::File::create(path).await.map_err(AppError::io)?;
        let mut hash = Sha256::new();
        let mut received = 0_u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| unreachable(&error))?
        {
            received += chunk.len() as u64;
            if received > expected {
                return Err(self.mismatch(&format!("more than {expected} bytes")));
            }
            hash.update(&chunk);
            file.write_all(&chunk).await.map_err(AppError::io)?;
        }
        if received != expected {
            return Err(unreachable(&format!(
                "the download ended after {received} of {expected} bytes"
            )));
        }
        file.sync_all().await.map_err(AppError::io)?;
        let actual = hex::encode(hash.finalize());
        if actual != self.asset.sha256 {
            return Err(self.mismatch(&format!(
                "SHA-256 {actual} instead of the pinned {}",
                self.asset.sha256
            )));
        }
        Ok(received)
    }

    fn mismatch(&self, detail: &str) -> AppError {
        install_error(format!(
            "the downloaded {} does not match the pinned Qdrant {VERSION} release ({detail}); it was discarded and nothing was installed. Retry later, and report it if this persists",
            self.asset.name
        ))
    }
}

impl Staged {
    /// Move the verified executable to `binary`, whose folder must not hold
    /// a working copy; a folder without one is replaced.
    pub(crate) fn place(self, binary: &Path) -> Result<(), AppError> {
        let folder = binary
            .parent()
            .ok_or_else(|| AppError::internal("binary without a folder"))?;
        if folder.exists() {
            std::fs::remove_dir_all(folder).map_err(AppError::io)?;
        }
        let parent = folder
            .parent()
            .ok_or_else(|| AppError::internal("binary folder without a parent"))?;
        super::create_private_directory(parent)?;
        let unpacked = self.directory.path().join("unpacked");
        super::create_private_directory(&unpacked)?;
        std::fs::rename(
            self.directory.path().join(self.executable),
            unpacked.join(self.executable),
        )
        .map_err(AppError::io)?;
        std::fs::rename(&unpacked, folder).map_err(AppError::io)?;
        crate::platform::sync_directory(parent).map_err(AppError::io)
    }
}

/// Copy the archive's one executable named `executable` to `target`.
pub(super) fn unpack(
    archive: &Path,
    format: Format,
    executable: &str,
    target: &Path,
) -> Result<(), AppError> {
    let file = std::fs::File::open(archive).map_err(AppError::io)?;
    let missing = || {
        install_error(format!(
            "the Qdrant archive holds no {executable}; nothing was installed"
        ))
    };
    let mut output = executable_file(target)?;
    let copied = match format {
        Format::TarGz => {
            let mut entries = tar::Archive::new(flate2::read::GzDecoder::new(file));
            let mut found = None;
            for entry in entries.entries().map_err(corrupt)? {
                let entry = entry.map_err(corrupt)?;
                let named = entry
                    .path()
                    .map_err(corrupt)?
                    .file_name()
                    .is_some_and(|name| name == executable);
                if named && entry.header().entry_type().is_file() {
                    found = Some(copy_bounded(entry, &mut output)?);
                    break;
                }
            }
            found.ok_or_else(missing)?
        }
        Format::Zip => {
            let mut zip = zip::ZipArchive::new(file).map_err(corrupt)?;
            let name = zip
                .file_names()
                .filter_map(Result::ok)
                .find(|name| {
                    Path::new(name.as_ref())
                        .file_name()
                        .is_some_and(|name| name == executable)
                })
                .map(std::borrow::Cow::into_owned)
                .ok_or_else(missing)?;
            let entry = zip.by_name(&name).map_err(corrupt)?;
            copy_bounded(entry, &mut output)?
        }
    };
    if copied == 0 {
        return Err(missing());
    }
    output.sync_all().map_err(AppError::io)
}

fn executable_file(path: &Path) -> Result<std::fs::File, AppError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o755);
    }
    options.open(path).map_err(AppError::io)
}

fn copy_bounded(entry: impl std::io::Read, output: &mut std::fs::File) -> Result<u64, AppError> {
    let copied =
        std::io::copy(&mut entry.take(MAX_EXECUTABLE_BYTES + 1), output).map_err(corrupt)?;
    if copied > MAX_EXECUTABLE_BYTES {
        return Err(install_error(
            "the Qdrant archive's executable is implausibly large; nothing was installed",
        ));
    }
    Ok(copied)
}

fn corrupt(error: impl std::fmt::Display) -> AppError {
    install_error(format!(
        "could not unpack the Qdrant archive ({error}); nothing was installed"
    ))
}
