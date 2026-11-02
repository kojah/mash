//! Bounded, message-linked attachment retention. Provider paths never become read APIs.
use crate::AppError;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(crate) const MAX_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_FILES: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    #[serde(default)]
    pub(crate) share_files: bool,
    /// Only explicit provider attachment references within these roots can be retained.
    #[serde(default)]
    pub(crate) allowed_roots: Vec<PathBuf>,
}

impl Configuration {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if self.allowed_roots.len() > 32 || self.allowed_roots.iter().any(|p| !p.is_absolute()) {
            return Err(AppError::configuration(
                "attachments.allowed_roots must contain at most 32 absolute paths",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Source {
    Retained {
        metadata: Metadata,
    },
    Base64 {
        name: String,
        media_type: String,
        data: String,
    },
    File {
        name: String,
        media_type: String,
        path: PathBuf,
    },
    External {
        name: String,
        media_type: String,
    },
}

impl Source {
    pub(crate) fn retained_size(&self) -> usize {
        match self {
            Self::Base64 {
                name,
                media_type,
                data,
            } => name
                .capacity()
                .saturating_add(media_type.capacity())
                .saturating_add(data.capacity()),
            Self::File {
                name,
                media_type,
                path,
            } => name
                .capacity()
                .saturating_add(media_type.capacity())
                .saturating_add(path.as_os_str().len()),
            Self::External { name, media_type } => {
                name.capacity().saturating_add(media_type.capacity())
            }
            Self::Retained { metadata } => metadata
                .name
                .capacity()
                .saturating_add(metadata.media_type.capacity())
                .saturating_add(128),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Availability {
    NotUploaded,
    Retained,
    Missing,
    NotAllowed,
    TooLarge,
    Invalid,
    External,
}

pub(crate) fn validate_metadata(records: &[Metadata]) -> Result<(), AppError> {
    let mut ids = std::collections::HashSet::new();
    if records.len() > MAX_FILES
        || records.iter().any(|m| {
            !ids.insert(&m.id)
                || !valid_digest(&m.id)
                || m.name.len() > 1024
                || m.media_type.len() > 128
                || m.digest.as_deref().is_some_and(|d| !valid_digest(d))
                || m.bytes.is_some_and(|n| n > MAX_BYTES as u64)
                || (m.availability == Availability::Retained
                    && (m.digest.is_none() || m.bytes.is_none()))
        })
    {
        return Err(AppError::usage("invalid attachment metadata"));
    }
    Ok(())
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Persist already-decoded attachment bytes. Binary `/v1/sync` uploads use
/// this path directly; HTTP JSON uploads decode base64 first.
pub(crate) fn write_bytes(root: &Path, metadata: &Metadata, bytes: &[u8]) -> Result<(), AppError> {
    validate_metadata(std::slice::from_ref(metadata))?;
    if metadata.availability != Availability::Retained {
        return Err(AppError::usage("invalid attachment upload"));
    }
    if bytes.len() > MAX_BYTES
        || metadata.bytes != Some(bytes.len() as u64)
        || metadata.digest.as_deref() != Some(blake3::hash(bytes).to_hex().as_str())
    {
        return Err(AppError::usage("attachment integrity mismatch"));
    }
    if let Ok(text) = std::str::from_utf8(bytes)
        && crate::redaction::text(text) != text
    {
        return Err(AppError::usage(
            "attachment text must be redacted before upload",
        ));
    }
    fs::create_dir_all(root).map_err(AppError::io)?;
    let mut file = tempfile::NamedTempFile::new_in(root).map_err(AppError::io)?;
    std::io::Write::write_all(&mut file, bytes).map_err(AppError::io)?;
    file.as_file().sync_all().map_err(AppError::io)?;
    file.persist(
        root.join(
            metadata
                .digest
                .as_deref()
                .ok_or_else(|| AppError::usage("missing attachment digest"))?,
        ),
    )
    .map_err(|e| AppError::io(e.error))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Metadata {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) media_type: String,
    pub(crate) availability: Availability,
    pub(crate) bytes: Option<u64>,
    pub(crate) digest: Option<String>,
    /// Text is retained as a redacted UTF-8 view, never the original secret-bearing bytes.
    pub(crate) redacted: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Listing {
    pub(crate) message_id: String,
    pub(crate) attachments: Vec<Metadata>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Retrieved {
    pub(crate) message_id: String,
    pub(crate) attachment: Metadata,
    pub(crate) data: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "wire identities distinguish publishing node, message and attachment"
)]
pub(crate) struct Request {
    pub(crate) node_id: String,
    pub(crate) message_id: String,
    pub(crate) attachment_id: Option<String>,
}

pub(crate) fn directory(database: &Path) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(".attachments");
    PathBuf::from(path)
}

fn clean_name(name: &str) -> String {
    crate::redaction::text(
        &name
            .chars()
            .filter(|c| !c.is_control())
            .take(255)
            .collect::<String>(),
    )
}

pub(crate) fn retain(
    root: &Path,
    config: &Configuration,
    message_id: &str,
    sources: &[Source],
) -> Result<Vec<Metadata>, AppError> {
    sources
        .iter()
        .take(MAX_FILES)
        .enumerate()
        .map(|(ordinal, source)| {
            if let Source::Retained { metadata } = source {
                return Ok(metadata.clone());
            }
            let (name, media_type) = match source {
                Source::Base64 {
                    name, media_type, ..
                }
                | Source::File {
                    name, media_type, ..
                }
                | Source::External { name, media_type } => (name, media_type),
                Source::Retained { metadata } => (&metadata.name, &metadata.media_type),
            };
            let mut metadata = Metadata {
                id: blake3::hash(format!("{message_id}\0{ordinal}").as_bytes())
                    .to_hex()
                    .to_string(),
                name: clean_name(name),
                media_type: media_type
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(128)
                    .collect(),
                availability: Availability::External,
                bytes: None,
                digest: None,
                redacted: false,
            };
            let result = match source {
                Source::Base64 { data, .. } => {
                    if data.len() > MAX_BYTES.div_ceil(3) * 4 {
                        Err(Availability::TooLarge)
                    } else {
                        STANDARD.decode(data).map_err(|_| Availability::Invalid)
                    }
                }
                Source::File { path, .. } => read_source(path, config),
                Source::External { .. } => Err(Availability::External),
                Source::Retained { .. } => Err(Availability::Invalid),
            };
            let mut bytes = match result {
                Ok(bytes) if bytes.len() <= MAX_BYTES => bytes,
                Ok(_) => {
                    metadata.availability = Availability::TooLarge;
                    return Ok(metadata);
                }
                Err(state) => {
                    metadata.availability = state;
                    return Ok(metadata);
                }
            };
            // Detect UTF-8 independently of provider MIME declarations. Binary images/PDFs
            // are not sanitized; neither OCR nor document execution occurs here.
            if let Ok(text) = std::str::from_utf8(&bytes) {
                bytes = crate::redaction::text(text).into_bytes();
                metadata.redacted = true;
            }
            if bytes.len() > MAX_BYTES {
                metadata.availability = Availability::TooLarge;
                return Ok(metadata);
            }
            let digest = blake3::hash(&bytes).to_hex().to_string();
            let path = root.join(&digest);
            // Files are content-addressed and only ever appear by atomic rename,
            // so an existing one already holds these bytes. Rewriting it on every
            // refresh of a growing transcript would cost writes and churn the cache.
            if !path.is_file() {
                fs::create_dir_all(root).map_err(AppError::io)?;
                // Atomic replace prevents concurrent indexing from serving partially written files.
                let mut file = tempfile::NamedTempFile::new_in(root).map_err(AppError::io)?;
                std::io::Write::write_all(&mut file, &bytes).map_err(AppError::io)?;
                file.persist(&path).map_err(|e| AppError::io(e.error))?;
            }
            metadata.availability = Availability::Retained;
            metadata.bytes = Some(bytes.len() as u64);
            metadata.digest = Some(digest);
            Ok(metadata)
        })
        .collect()
}

fn read_source(path: &Path, config: &Configuration) -> Result<Vec<u8>, Availability> {
    let path = fs::canonicalize(path).map_err(|_| Availability::Missing)?;
    if !config
        .allowed_roots
        .iter()
        .filter_map(|root| fs::canonicalize(root).ok())
        .any(|root| path.starts_with(root))
    {
        return Err(Availability::NotAllowed);
    }
    let info = fs::metadata(&path).map_err(|_| Availability::Missing)?;
    if !info.is_file() {
        return Err(Availability::Invalid);
    }
    if info.len() > MAX_BYTES as u64 {
        return Err(Availability::TooLarge);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| Availability::Missing)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Availability::Missing)?;
    Ok(bytes)
}

pub(crate) fn read(root: &Path, metadata: &Metadata) -> Result<String, AppError> {
    if metadata.availability != Availability::Retained {
        return Err(AppError::usage(
            "attachment bytes were not retained; inspect its availability",
        ));
    }
    let digest = metadata
        .digest
        .as_deref()
        .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| AppError::database_data("invalid attachment digest"))?;
    let mut bytes = Vec::new();
    fs::File::open(root.join(digest))
        .map_err(|_| AppError::usage("retained attachment file is missing; reindex its source"))?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(AppError::io)?;
    if bytes.len() > MAX_BYTES
        || metadata.bytes != Some(bytes.len() as u64)
        || blake3::hash(&bytes).to_hex().as_str() != digest
    {
        return Err(AppError::database_data(
            "retained attachment failed integrity validation",
        ));
    }
    if let Ok(text) = std::str::from_utf8(&bytes) {
        bytes = crate::redaction::text(text).into_bytes();
    }
    Ok(STANDARD.encode(bytes))
}

pub(crate) fn url(name: &str, url: &str) -> Source {
    if let Some(data) = url
        .strip_prefix("data:")
        .and_then(|s| s.split_once(";base64,"))
    {
        Source::Base64 {
            name: name.into(),
            media_type: data.0.into(),
            data: data.1.into(),
        }
    } else if let Ok(url) = reqwest::Url::parse(url)
        && let Ok(path) = url.to_file_path()
    {
        Source::File {
            name: name.into(),
            media_type: "application/octet-stream".into(),
            path,
        }
    } else {
        Source::External {
            name: name.into(),
            media_type: "application/octet-stream".into(),
        }
    }
}

pub(crate) fn local_image(path: &Path) -> Source {
    let media_type = match path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    Source::File {
        name: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("image")
            .into(),
        media_type: media_type.into(),
        path: path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests;
