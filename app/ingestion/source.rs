//! Physical histories and the logical sessions they contain have distinct identities.
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceIdentity {
    pub(crate) path: PathBuf,
    pub(crate) session: Option<String>,
}

/// Scan metadata changes independently of a source's identity.
#[derive(Clone)]
pub(crate) struct HistorySource {
    pub(crate) identity: SourceIdentity,
    pub(crate) activity_ns: Option<i64>,
    pub(crate) estimated_bytes: Option<u64>,
}

impl HistorySource {
    pub(crate) fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            identity: SourceIdentity::file(path),
            activity_ns: None,
            estimated_bytes: None,
        }
    }

    pub(crate) fn stamp(&self) -> Result<super::SourceStamp, crate::AppError> {
        let mut stamp = super::source_stamp_for(&self.identity)?;
        stamp.activity_ns = self.activity_ns;
        Ok(stamp)
    }
}

impl SourceIdentity {
    pub(crate) fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            session: None,
        }
    }

    pub(crate) fn key(&self) -> String {
        let path = self.path.to_string_lossy();
        match &self.session {
            None => format!("file:{path}"),
            Some(session) => {
                // Length delimiters prevent collisions between path/session pairs.
                let mut hash = blake3::Hasher::new();
                for value in [path.as_bytes(), session.as_bytes()] {
                    hash.update(&(value.len() as u64).to_le_bytes());
                    hash.update(value);
                }
                format!("session:{}", hash.finalize().to_hex())
            }
        }
    }

    pub(crate) fn publication_hash(&self) -> String {
        let identity = match self.session {
            None => format!("search-text-v3:{}", self.path.to_string_lossy()),
            Some(_) => format!("search-text-v3-session:{}", self.key()),
        };
        blake3::hash(identity.as_bytes()).to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keys_are_stable_separate_from_paths_and_unambiguous() {
        let file = SourceIdentity::file("/tmp/history.db");
        let first = SourceIdentity {
            session: Some("alpha".into()),
            ..file.clone()
        };
        let second = SourceIdentity {
            session: Some("beta".into()),
            ..file.clone()
        };
        assert_eq!(first.path, file.path);
        assert_eq!(first.key(), first.clone().key());
        assert_ne!(first.key(), second.key());
        assert_ne!(first.key(), file.key());
        assert_ne!(first.publication_hash(), file.publication_hash());
        assert_ne!(first.publication_hash(), second.publication_hash());
        let other_database = SourceIdentity {
            path: "/tmp/other.db".into(),
            ..first.clone()
        };
        assert_ne!(first.key(), other_database.key());
        let ambiguous_left = SourceIdentity {
            path: "a:b".into(),
            session: Some("c".into()),
        };
        let ambiguous_right = SourceIdentity {
            path: "a".into(),
            session: Some("b:c".into()),
        };
        assert_ne!(ambiguous_left.key(), ambiguous_right.key());
    }
}
