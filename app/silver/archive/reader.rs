use super::Record;
use crate::AppError;
use std::io::{BufRead, Read};

const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) struct Reader<R> {
    input: R,
    line: Vec<u8>,
    state_seen: bool,
    ended: bool,
    failed: bool,
    bytes: u64,
    hash: blake3::Hasher,
}

impl<R: BufRead> Reader<R> {
    pub(crate) fn new(input: R) -> Self {
        Self {
            input,
            line: Vec::new(),
            state_seen: false,
            ended: false,
            failed: false,
            bytes: 0,
            hash: blake3::Hasher::new(),
        }
    }

    pub(crate) fn finish(self) -> Result<(u64, String), AppError> {
        if !self.ended || self.failed || !self.state_seen {
            return Err(AppError::usage("incomplete canonical archive"));
        }
        Ok((self.bytes, self.hash.finalize().to_hex().to_string()))
    }

    fn read(&mut self) -> Result<Option<Record>, AppError> {
        self.line.clear();
        let bytes = (&mut self.input)
            .take(MAX_RECORD_BYTES + 1)
            .read_until(b'\n', &mut self.line)
            .map_err(AppError::io)?;
        if bytes == 0 {
            self.ended = true;
            if !self.state_seen {
                return Err(AppError::usage("incomplete canonical archive"));
            }
            return Ok(None);
        }
        if bytes as u64 > MAX_RECORD_BYTES || self.line.last() != Some(&b'\n') || self.state_seen {
            return Err(AppError::usage(
                "invalid or oversized canonical archive record",
            ));
        }
        let record: Record = serde_json::from_slice(&self.line)
            .map_err(|_| AppError::usage("invalid canonical archive record"))?;
        record.validate()?;
        self.state_seen = matches!(record, Record::State { .. });
        self.bytes = self
            .bytes
            .checked_add(bytes as u64)
            .ok_or_else(|| AppError::usage("canonical archive size overflow"))?;
        self.hash.update(&self.line);
        Ok(Some(record))
    }
}

impl<R: BufRead> Iterator for Reader<R> {
    type Item = Result<Record, AppError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.ended || self.failed {
            return None;
        }
        match self.read() {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => None,
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }
}
