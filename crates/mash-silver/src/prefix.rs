//! Seekable JSONL captures retain original identities while permitting later appends.
use super::SourceStamp;
use crate::AppError;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix {
    identity: Arc<same_file::Handle>,
    length: u64,
    hash: blake3::Hash,
}

pub fn capture(path: &Path, stamp: &SourceStamp) -> Result<(File, Prefix), AppError> {
    let mut source = File::open(path).map_err(AppError::io)?;
    let identity = Arc::new(
        same_file::Handle::from_file(source.try_clone().map_err(AppError::io)?)
            .map_err(AppError::io)?,
    );
    stamp.verify(&super::SourceIdentity::file(path))?;
    let length = u64::try_from(stamp.size_bytes)
        .map_err(|_| AppError::internal("invalid JSONL capture size"))?;
    let mut file = tempfile::tempfile().map_err(AppError::io)?;
    let mut hash = blake3::Hasher::new();
    let mut remaining = length;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    while remaining > 0 {
        let amount = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| AppError::internal("invalid JSONL read size"))?;
        let read = source.read(&mut buffer[..amount]).map_err(AppError::io)?;
        if read == 0 {
            return Err(AppError::source_changed());
        }
        file.write_all(&buffer[..read]).map_err(AppError::io)?;
        hash.update(&buffer[..read]);
        remaining -= read as u64;
    }
    file.rewind().map_err(AppError::io)?;
    Ok((
        file,
        Prefix {
            identity,
            length,
            hash: hash.finalize(),
        },
    ))
}

impl Prefix {
    pub fn verify(&self, path: &Path) -> Result<(), AppError> {
        let file = File::open(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                AppError::source_changed()
            } else {
                AppError::io(error)
            }
        })?;
        let identity = same_file::Handle::from_file(file.try_clone().map_err(AppError::io)?)
            .map_err(AppError::io)?;
        if identity != *self.identity || file.metadata().map_err(AppError::io)?.len() < self.length
        {
            return Err(AppError::source_changed());
        }
        let mut hash = blake3::Hasher::new();
        hash.update_reader(file.take(self.length))
            .map_err(AppError::io)?;
        if hash.finalize() != self.hash {
            return Err(AppError::source_changed());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_is_bounded_and_only_unchanged_prefixes_survive() {
        for mutation in ["append", "edit", "truncate", "replace", "remove"] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("history.jsonl");
            std::fs::write(&path, "original\n").unwrap();
            let stamp = crate::source_stamp(&path).unwrap();
            let (mut file, proof) = capture(&path, &stamp).unwrap();
            match mutation {
                "append" => std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap()
                    .write_all(b"later\n")
                    .unwrap(),
                "edit" => std::fs::write(&path, "replaced\nlater\n").unwrap(),
                "truncate" => std::fs::write(&path, "short").unwrap(),
                "replace" => {
                    let new = root.path().join("new");
                    std::fs::write(&new, "original\nlater\n").unwrap();
                    std::fs::rename(new, &path).unwrap();
                }
                "remove" => std::fs::remove_file(&path).unwrap(),
                _ => unreachable!(),
            }
            let mut captured = String::new();
            file.read_to_string(&mut captured).unwrap();
            assert_eq!(captured, "original\n");
            let verified = proof.verify(&path);
            if mutation == "append" {
                assert!(verified.is_ok());
            } else {
                assert_eq!(
                    verified.unwrap_err().error.kind,
                    crate::ErrorKind::SourceChanged
                );
            }
        }
    }

    #[test]
    fn parsers_keep_original_identities_and_reject_malformed_captures() {
        let root = tempfile::tempdir().unwrap();
        for (provider, text) in [
            (
                crate::provider::Provider::Codex,
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\"}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"original\"}]}}\n",
            ),
            (
                crate::provider::Provider::ClaudeCode,
                "{\"type\":\"user\",\"sessionId\":\"thread\",\"uuid\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"original\"}}\n",
            ),
        ] {
            let path = root.path().join(format!("{}.jsonl", provider.as_str()));
            std::fs::write(&path, text).unwrap();
            let expected = crate::Parser::Native(provider)
                .parse(&crate::HistorySource::file(&path))
                .unwrap()
                .chat
                .unwrap();
            let stamp = crate::source_stamp(&path).unwrap();
            let (file, proof) = capture(&path, &stamp).unwrap();
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(b"malformed appended record\n")
                .unwrap();
            proof.verify(&path).unwrap();
            let captured = crate::parsers::parse_captured(&path, provider, file)
                .unwrap()
                .chat
                .unwrap();
            assert_eq!(captured.id, expected.id);
            assert_eq!(captured.source, expected.source);
            assert_eq!(
                captured.messages.inline()[0].id,
                expected.messages.inline()[0].id
            );
            assert_eq!(captured.messages.len(), 1);
            let mut stamp = crate::source_stamp(&path).unwrap();
            let parsed = crate::parse_stable_source(
                &crate::HistorySource::file(&path),
                &mut stamp,
                crate::Parser::Native(provider),
            )
            .unwrap();
            assert_eq!(parsed.malformed_records, 1);
            assert!(parsed.chat.is_none());
        }
    }
}
