//! Offline, versioned local silver exports. View and deployment state are rebuilt.
use crate::{
    AppError, attachments,
    silver::certificate::Certificate,
    storage::{self, Storage},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const FORMAT_VERSION: u32 = 2;
const DATABASE: &str = "silver.jsonl";
const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileDigest {
    bytes: u64,
    blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryCounts {
    pub chats: u64,
    pub messages: u64,
    pub attachment_files: u64,
    pub attachment_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: Format,
    format_version: u32,
    record_schema_version: u32,
    created_at: String,
    source_node: Option<String>,
    pub silver: Certificate,
    counts: HistoryCounts,
    records: FileDigest,
}

#[derive(Debug, Deserialize, Serialize)]
enum Format {
    #[serde(rename = "mash-local-silver")]
    LocalSilver,
}

#[derive(Debug, Serialize)]
pub struct Exported {
    pub directory: PathBuf,
    pub format_version: u32,
    pub silver: Certificate,
    pub counts: HistoryCounts,
}

#[derive(Debug, Serialize)]
pub struct Imported {
    pub database: PathBuf,
    pub silver: Certificate,
    pub counts: HistoryCounts,
    pub search_ready: bool,
    pub recommended_action: NextStep,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NextStep {
    RebuildLocalViews,
    RepairSilver,
}

pub fn write(
    database: &Path,
    directory: &Path,
    source_node: Option<String>,
) -> Result<Exported, AppError> {
    let mut output = OutputDirectory::create(directory)?;
    let snapshot = directory.join(DATABASE);
    let captured = storage::export::write(database, &snapshot)?;
    let files = captured.files;
    let blobs = directory.join("attachments");
    let _blob_dir = OutputDirectory::create(&blobs)?.keep();
    copy_attachments(&attachments::directory(database), &blobs, &files)?;
    sync_directory(&blobs)?;
    let silver = captured.silver;
    let counts = history_counts(&captured.counts, &files)?;
    let manifest = Manifest {
        format: Format::LocalSilver,
        format_version: FORMAT_VERSION,
        record_schema_version: crate::silver::archive::VERSION,
        created_at: chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
            .to_rfc3339(),
        source_node,
        silver: silver.clone(),
        counts: counts.clone(),
        records: digest(&snapshot)?,
    };
    // The completion marker is written last; interrupted exports cannot import.
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| AppError::internal("serialize export manifest"))?;
    crate::platform::sync_file(&snapshot).map_err(AppError::io)?;
    write_new(&directory.join(MANIFEST), &json)?;
    sync_directory(directory)?;
    output.complete = true;
    Ok(Exported {
        directory: directory.to_path_buf(),
        format_version: FORMAT_VERSION,
        silver,
        counts,
    })
}

pub fn load(directory: &Path, database: &Path, keep_identity: bool) -> Result<Imported, AppError> {
    let manifest: Manifest =
        serde_json::from_slice(&read_limited(&directory.join(MANIFEST), 64 * 1024)?)
            .map_err(|_| AppError::usage("invalid or incomplete export manifest"))?;
    if manifest.format_version != FORMAT_VERSION
        || ![2, 3, crate::silver::archive::VERSION].contains(&manifest.record_schema_version)
    {
        return Err(AppError::schema(
            "unsupported export format/schema; use a compatible Mash build",
        ));
    }
    ensure_absent(database)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut path = database.as_os_str().to_owned();
        path.push(suffix);
        ensure_absent(Path::new(&path))?;
    }
    let blobs = attachments::directory(database);
    ensure_absent(&blobs)?;
    // Stale views could pair with a database that keeps its identity.
    let parent = parent(database);
    fs::create_dir_all(parent).map_err(AppError::io)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".mash-import-");
    crate::platform::private_tempdir(&mut builder);
    let stage = builder.tempdir_in(parent).map_err(AppError::io)?;
    let input = stage.path().join("input.jsonl");
    copy_checked(&directory.join(DATABASE), &input, &manifest.records)?;
    let output = stage.path().join("imported.sqlite3");
    drop(crate::scratch::sqlite(&output)?);
    storage::export::restore(&input, &output, keep_identity)?;
    let history = Storage::open_silver(&output)?;
    storage::export::validate(&history)?;
    let files = storage::export::attachments(&history)?;
    if !storage::export::certificate_matches(&history, &manifest.silver)?
        || counts(&history, &files)? != manifest.counts
    {
        return Err(AppError::database_data(
            "export manifest does not match silver history",
        ));
    }
    let silver = history.silver_certificate()?;
    drop(history);
    let mut blob_output = OutputDirectory::create(&blobs)?;
    copy_attachments(&directory.join("attachments"), &blobs, &files)?;
    sync_directory(&blobs)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(AppError::io)?;
    std::io::copy(
        &mut fs::File::open(output).map_err(AppError::io)?,
        file.as_file_mut(),
    )
    .map_err(AppError::io)?;
    file.as_file().sync_all().map_err(AppError::io)?;
    // Database installation is last, atomic and never overwrites an existing DB.
    file.persist_noclobber(database)
        .map_err(|e| AppError::io(e.error))?;
    blob_output.complete = true;
    sync_directory(parent)?;
    let recommended_action = if silver.is_complete() {
        NextStep::RebuildLocalViews
    } else {
        NextStep::RepairSilver
    };
    Ok(Imported {
        database: database.to_path_buf(),
        silver,
        counts: manifest.counts,
        search_ready: false,
        recommended_action,
    })
}

fn counts(storage: &Storage, files: &BTreeMap<String, u64>) -> Result<HistoryCounts, AppError> {
    let counts = storage.silver_counts()?;
    history_counts(&counts, files)
}

fn history_counts(
    counts: &crate::storage::Counts,
    files: &BTreeMap<String, u64>,
) -> Result<HistoryCounts, AppError> {
    Ok(HistoryCounts {
        chats: counts.chats,
        messages: counts.messages,
        attachment_files: u64::try_from(files.len())
            .map_err(|_| AppError::internal("too many export attachments"))?,
        attachment_bytes: files
            .values()
            .try_fold(0_u64, |total, bytes| total.checked_add(*bytes))
            .ok_or_else(|| AppError::database_data("export attachment size overflow"))?,
    })
}

fn copy_attachments(
    source: &Path,
    target: &Path,
    files: &BTreeMap<String, u64>,
) -> Result<(), AppError> {
    for (hash, &bytes) in files {
        copy_checked(
            &source.join(hash),
            &target.join(hash),
            &FileDigest {
                bytes,
                blake3: hash.clone(),
            },
        )?;
    }
    Ok(())
}

fn copy_checked(source: &Path, target: &Path, expected: &FileDigest) -> Result<(), AppError> {
    require_regular_file(source)?;
    // Check the bytes actually copied, not a pre-copy checksum of mutable input.
    let mut input = fs::File::open(source)
        .map_err(AppError::io)?
        .take(expected.bytes.saturating_add(1));
    let mut output = tempfile::NamedTempFile::new_in(parent(target)).map_err(AppError::io)?;
    let bytes = std::io::copy(&mut input, output.as_file_mut()).map_err(AppError::io)?;
    if bytes != expected.bytes || digest(output.path())? != *expected {
        return Err(AppError::database_data(
            "export file checksum or size mismatch",
        ));
    }
    output.as_file().sync_all().map_err(AppError::io)?;
    output
        .persist_noclobber(target)
        .map_err(|e| AppError::io(e.error))?;
    Ok(())
}

fn digest(path: &Path) -> Result<FileDigest, AppError> {
    let mut reader = fs::File::open(path).map_err(AppError::io)?;
    let mut hash = blake3::Hasher::new();
    let mut bytes = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let length = reader.read(&mut buffer).map_err(AppError::io)?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
        bytes = bytes
            .checked_add(length as u64)
            .ok_or_else(|| AppError::database_data("export file size overflow"))?;
    }
    Ok(FileDigest {
        bytes,
        blake3: hash.finalize().to_hex().to_string(),
    })
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, AppError> {
    require_regular_file(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(AppError::io)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(AppError::io)?;
    if bytes.len() > limit {
        return Err(AppError::usage("export manifest exceeds size limit"));
    }
    Ok(bytes)
}

fn require_regular_file(path: &Path) -> Result<(), AppError> {
    if !fs::symlink_metadata(path)
        .map_err(AppError::io)?
        .file_type()
        .is_file()
    {
        return Err(AppError::usage(
            "export input must be a regular file, not a symlink",
        ));
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let mut file = tempfile::NamedTempFile::new_in(parent(path)).map_err(AppError::io)?;
    file.write_all(bytes).map_err(AppError::io)?;
    file.as_file().sync_all().map_err(AppError::io)?;
    file.persist_noclobber(path)
        .map_err(|e| AppError::io(e.error))?;
    Ok(())
}

fn ensure_absent(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::io(error)),
        Ok(_) => Err(AppError::usage(
            "export/import destination already exists; choose a new path",
        )),
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn sync_directory(path: &Path) -> Result<(), AppError> {
    crate::platform::sync_directory(path).map_err(AppError::io)
}

struct OutputDirectory {
    path: PathBuf,
    complete: bool,
}

impl OutputDirectory {
    fn create(path: &Path) -> Result<Self, AppError> {
        fs::create_dir_all(parent(path)).map_err(AppError::io)?;
        let mut builder = fs::DirBuilder::new();
        crate::platform::private_directory(&mut builder);
        builder.create(path).map_err(AppError::io)?;
        Ok(Self {
            path: path.to_path_buf(),
            complete: false,
        })
    }

    fn keep(mut self) -> Self {
        self.complete = true;
        self
    }
}

impl Drop for OutputDirectory {
    fn drop(&mut self) {
        if !self.complete {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests;
