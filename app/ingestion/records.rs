//! Bound allocations before decoding line-oriented provider input.
use crate::AppError;
use std::io::{BufRead, Read};

mod json;
pub(super) use json::decode;

pub(super) struct Lines<R> {
    reader: R,
    finished: bool,
}

impl<R: BufRead> Lines<R> {
    pub(super) fn new(reader: R) -> Self {
        Self {
            reader,
            finished: false,
        }
    }
}

impl<R: BufRead> Iterator for Lines<R> {
    type Item = Result<String, AppError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let result = read_line(&mut self.reader, super::MAX_RECORD_BYTES);
        match result {
            Ok(Some(line)) => Some(Ok(line)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}

fn read_line(reader: &mut impl BufRead, limit: usize) -> Result<Option<String>, AppError> {
    // Allow LF/CRLF in addition to the payload, plus one byte to distinguish
    // an oversized line from a valid line at the boundary. Never drain the
    // remaining input after failure: indexing must reject the whole source.
    let mut bytes = Vec::new();
    let count = reader
        .take((limit + 3) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(AppError::io)?;
    if count == 0 {
        return Ok(None);
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    if bytes.len() > limit {
        return Err(AppError::configuration(
            "provider record exceeds the 8 MiB ingestion limit",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| AppError::configuration("provider history contains invalid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn accepts_boundary_payloads_and_native_line_endings() {
        for ending in ["", "\n", "\r\n"] {
            let mut input = Cursor::new(format!("éé{ending}"));
            assert_eq!(read_line(&mut input, 4).unwrap().as_deref(), Some("éé"));
            assert!(read_line(&mut input, 4).unwrap().is_none());
        }
        let mut lines = Lines::new(Cursor::new(b"\n\r\nhello\r\nlast"));
        assert_eq!(lines.next().unwrap().unwrap(), "");
        assert_eq!(lines.next().unwrap().unwrap(), "");
        assert_eq!(lines.next().unwrap().unwrap(), "hello");
        assert_eq!(lines.next().unwrap().unwrap(), "last");
        assert!(lines.next().is_none());
    }

    #[test]
    fn rejects_oversized_input_without_reading_the_whole_record() {
        for ending in ["", "\n", "\r\n"] {
            let mut input = Cursor::new(format!("{}{ending}", "x".repeat(1000)));
            let error = read_line(&mut input, 4).unwrap_err();
            assert!(error.error.message.contains("ingestion limit"));
            assert_eq!(input.position(), 7);
        }
        let mut input = Cursor::new("ééé\n");
        assert!(
            read_line(&mut input, 4).is_err(),
            "limit counts UTF-8 bytes"
        );
    }

    #[test]
    fn invalid_utf8_stops_the_source_instead_of_skipping_a_record() {
        let mut lines = Lines::new(Cursor::new(b"valid\n\xff\nlater\n"));
        assert_eq!(lines.next().unwrap().unwrap(), "valid");
        assert!(
            lines
                .next()
                .unwrap()
                .unwrap_err()
                .error
                .message
                .contains("UTF-8")
        );
        assert!(lines.next().is_none());
    }
}
