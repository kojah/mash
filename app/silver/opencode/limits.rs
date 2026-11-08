//! Bound a session before fetching SQLite text and retaining normalized copies.
use crate::AppError;

// Tool-heavy chats can retain more than 32 MiB without approaching the native
// input cap. Large sources already reserve the whole parser admission budget;
// keep a separate bounded allowance for canonical text and searchable copies.
pub(super) const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECORDS: usize = 65_536;

#[derive(Default)]
pub(super) struct Budget {
    input: usize,
    records: usize,
    retained: usize,
}

pub(super) fn invalid() -> AppError {
    AppError::configuration("OpenCode session exceeds the ingestion memory limit")
}

impl Budget {
    pub(super) fn read(&mut self, bytes: i64) -> Result<(), AppError> {
        let bytes = usize::try_from(bytes).map_err(|_| invalid())?;
        let input = self.input.checked_add(bytes).ok_or_else(invalid)?;
        if input > MAX_INPUT_BYTES || self.records == MAX_RECORDS {
            return Err(invalid());
        }
        self.input = input;
        self.records += 1;
        Ok(())
    }

    pub(super) fn retain(&mut self, bytes: usize) -> Result<(), AppError> {
        let retained = self.retained.checked_add(bytes).ok_or_else(invalid)?;
        if retained > MAX_RETAINED_BYTES {
            return Err(invalid());
        }
        self.retained = retained;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_and_retention_boundaries_reject_without_consuming_budget() {
        let mut budget = Budget::default();
        budget
            .read(i64::try_from(MAX_INPUT_BYTES).unwrap())
            .unwrap();
        assert!(budget.read(1).is_err());
        assert!(budget.read(-1).is_err());
        budget.retain(MAX_RETAINED_BYTES).unwrap();
        assert!(budget.retain(1).is_err());
        assert!(budget.retain(usize::MAX).is_err());
        assert_eq!(budget.input, MAX_INPUT_BYTES);
        assert_eq!(budget.retained, MAX_RETAINED_BYTES);
    }

    #[test]
    fn tiny_records_cannot_bypass_the_record_count_limit() {
        let mut budget = Budget::default();
        for _ in 0..MAX_RECORDS {
            budget.read(0).unwrap();
        }
        assert!(budget.read(0).is_err());
        assert_eq!(budget.records, MAX_RECORDS);
    }
}
