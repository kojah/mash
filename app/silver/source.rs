//! Physical histories and the logical sessions they contain have distinct identities.
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceIdentity {
    pub(crate) path: PathBuf,
    pub(crate) session: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Dependency {
    Required(PathBuf),
    Optional(PathBuf),
}

impl Dependency {
    pub(super) fn path(&self) -> &std::path::Path {
        match self {
            Self::Required(path) | Self::Optional(path) => path,
        }
    }
}

/// Scan metadata changes independently of a source's identity.
#[derive(Clone)]
pub(crate) struct HistorySource {
    pub(crate) identity: SourceIdentity,
    pub(crate) activity_ns: Option<i64>,
    pub(crate) estimated_bytes: Option<u64>,
    pub(crate) dependencies: Vec<Dependency>,
}

impl HistorySource {
    pub(crate) fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            identity: SourceIdentity::file(path),
            activity_ns: None,
            estimated_bytes: None,
            dependencies: Vec::new(),
        }
    }

    pub(crate) fn stamp(&self) -> Result<super::SourceStamp, crate::AppError> {
        let mut stamp = super::source_stamp_for(&self.identity)?;
        let activity = self.activity_ns.or_else(|| {
            (self.identity.session.is_none() && !self.dependencies.is_empty())
                .then_some(stamp.modified_ns)
        });
        stamp.include_dependencies(&self.dependencies)?;
        stamp.activity_ns = activity;
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
