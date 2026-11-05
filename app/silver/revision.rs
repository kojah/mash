//! Canonical revision identity, independent of publication and model state.
use crate::AppError;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    pub(crate) schema_version: u32,
    pub(crate) revision: u64,
    pub(crate) fingerprint: String,
}

impl State {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if self.schema_version != SCHEMA_VERSION
            || self.revision == 0
            || self.fingerprint.len() != 64
            || !self
                .fingerprint
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(AppError::shared("invalid shared silver revision"));
        }
        Ok(())
    }

    pub(crate) fn next(previous: Option<&Self>, fingerprint: String) -> Result<Self, AppError> {
        let revision = if let Some(previous) = previous {
            previous.validate()?;
            if previous.fingerprint == fingerprint {
                previous.revision
            } else {
                previous
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| AppError::shared("shared silver revision exhausted"))?
            }
        } else {
            1
        };
        let value = Self {
            schema_version: SCHEMA_VERSION,
            revision,
            fingerprint,
        };
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_invalid_digests_and_revision_exhaustion_fail_closed() {
        let original = State::next(None, "a".repeat(64)).unwrap();
        assert_eq!(
            State::next(Some(&original), original.fingerprint.clone()).unwrap(),
            original
        );
        for fingerprint in ["a".repeat(63), "A".repeat(64), "z".repeat(64)] {
            assert!(State::next(None, fingerprint).is_err());
        }
        let mut invalid = original.clone();
        invalid.schema_version += 1;
        assert!(State::next(Some(&invalid), original.fingerprint.clone()).is_err());
        invalid = original;
        invalid.revision = u64::MAX;
        assert!(State::next(Some(&invalid), "b".repeat(64)).is_err());
        assert_eq!(
            State::next(Some(&invalid), invalid.fingerprint.clone()).unwrap(),
            invalid
        );
    }
}
