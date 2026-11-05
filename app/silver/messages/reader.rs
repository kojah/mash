//! Length-framed typed metadata and raw UTF-8 payloads for private scratch data.
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use super::{NormalizedMessage, scratch_error};
use crate::AppError;
use crate::message_context::MessageContext;
use crate::role::Role;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    attachments: Vec<crate::attachments::Source>,
    id: String,
    ordinal: i64,
    context: MessageContext,
    role: Role,
    created_at: Option<i64>,
    content_bytes: u64,
    projection_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "crate::tools::Activity::is_empty")]
    tools: crate::tools::Activity,
}

pub(super) fn write_record(
    writer: &mut impl Write,
    message: &NormalizedMessage,
) -> Result<(), AppError> {
    let header = Header {
        attachments: message.attachments.clone(),
        id: message.id.clone(),
        ordinal: message.ordinal,
        context: message.context.clone(),
        role: message.role.clone(),
        created_at: message.created_at,
        content_bytes: message.content.len() as u64,
        projection_bytes: message.dialogue_text.as_ref().map(|text| text.len() as u64),
        tools: message.tools.clone(),
    };
    let header = serde_json::to_vec(&header).map_err(scratch_error)?;
    writer
        .write_all(&(header.len() as u64).to_le_bytes())
        .map_err(AppError::io)?;
    writer.write_all(&header).map_err(AppError::io)?;
    writer
        .write_all(message.content.as_bytes())
        .map_err(AppError::io)?;
    if let Some(projection) = &message.dialogue_text {
        writer
            .write_all(projection.as_bytes())
            .map_err(AppError::io)?;
    }
    let mut digest = blake3::Hasher::new();
    digest.update(&header);
    digest.update(message.content.as_bytes());
    if let Some(projection) = &message.dialogue_text {
        digest.update(projection.as_bytes());
    }
    writer
        .write_all(digest.finalize().as_bytes())
        .map_err(AppError::io)?;
    Ok(())
}

fn invalid() -> AppError {
    AppError::io(std::io::Error::other(
        "invalid normalized-message scratch record",
    ))
}

fn bytes(reader: &mut impl Read, length: u64) -> Result<Vec<u8>, AppError> {
    let length = usize::try_from(length).map_err(|_| invalid())?;
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).map_err(AppError::io)?;
    Ok(bytes)
}

fn record(reader: &mut impl Read, length: u64) -> Result<NormalizedMessage, AppError> {
    let mut prefix = [0; 8];
    reader.read_exact(&mut prefix).map_err(AppError::io)?;
    let header_length = u64::from_le_bytes(prefix);
    let available = length.checked_sub(8 + 32).ok_or_else(invalid)?;
    // Bounds come from the writer's immutable in-memory offsets, rather than
    // trusting lengths in a damaged file before allocating any payload buffer.
    if header_length > available {
        return Err(invalid());
    }
    let encoded_header = bytes(reader, header_length)?;
    let header: Header = serde_json::from_slice(&encoded_header).map_err(scratch_error)?;
    let payload_length = header
        .content_bytes
        .checked_add(header.projection_bytes.unwrap_or(0))
        .ok_or_else(invalid)?;
    if payload_length != available - header_length {
        return Err(invalid());
    }
    let content = String::from_utf8(bytes(reader, header.content_bytes)?).map_err(|_| invalid())?;
    let dialogue_text = header
        .projection_bytes
        .map(|length| String::from_utf8(bytes(reader, length)?).map_err(|_| invalid()))
        .transpose()?;
    let mut expected_digest = [0; 32];
    reader
        .read_exact(&mut expected_digest)
        .map_err(AppError::io)?;
    let mut digest = blake3::Hasher::new();
    digest.update(&encoded_header);
    digest.update(content.as_bytes());
    if let Some(projection) = &dialogue_text {
        digest.update(projection.as_bytes());
    }
    if digest.finalize().as_bytes() != &expected_digest {
        return Err(invalid());
    }
    Ok(NormalizedMessage {
        attachments: header.attachments,
        id: header.id,
        ordinal: header.ordinal,
        context: header.context,
        role: header.role,
        content,
        dialogue_text,
        created_at: header.created_at,
        tools: header.tools,
    })
}

pub(super) fn read_at(
    path: &Path,
    offsets: &[u64],
    index: usize,
) -> Result<Option<NormalizedMessage>, AppError> {
    let Some(&offset) = offsets.get(index) else {
        return Ok(None);
    };
    let mut file = File::open(path).map_err(AppError::io)?;
    let end = match offsets.get(index + 1) {
        Some(&end) => end,
        None => file.metadata().map_err(AppError::io)?.len(),
    };
    file.seek(SeekFrom::Start(offset)).map_err(AppError::io)?;
    record(
        &mut BufReader::new(file),
        end.checked_sub(offset).ok_or_else(invalid)?,
    )
    .map(Some)
}

pub(crate) struct Records {
    reader: BufReader<File>,
    offsets: Arc<[u64]>,
    length: u64,
    index: usize,
}

impl Records {
    pub(super) fn next(&mut self) -> Option<Result<NormalizedMessage, AppError>> {
        if self.index == self.offsets.len() {
            return match self.reader.fill_buf() {
                Ok([]) => None,
                Ok(_) => Some(Err(invalid())),
                Err(error) => Some(Err(AppError::io(error))),
            };
        }
        let start = self.offsets[self.index];
        let end = self
            .offsets
            .get(self.index + 1)
            .copied()
            .unwrap_or(self.length);
        self.index += 1;
        Some(
            end.checked_sub(start)
                .ok_or_else(invalid)
                .and_then(|length| record(&mut self.reader, length)),
        )
    }
}

pub(super) fn records(path: &Path, offsets: Arc<[u64]>, length: u64) -> Result<Records, AppError> {
    let file = File::open(path).map_err(AppError::io)?;
    Ok(Records {
        reader: BufReader::new(file),
        offsets,
        length,
        index: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_rejects_impossible_lengths_and_invalid_utf8_before_exposing_a_message() {
        let message = NormalizedMessage {
            attachments: Vec::new(),
            id: "id".to_owned(),
            ordinal: 0,
            context: MessageContext::Root,
            role: Role::User,
            created_at: None,
            content: "hello".to_owned(),
            dialogue_text: None,
            tools: crate::tools::Activity::call(
                Some("shell"),
                Some("call-1"),
                Some(&serde_json::json!({"command": ["ls"]})),
            ),
        };
        let mut encoded = Vec::new();
        write_record(&mut encoded, &message).unwrap();
        let length = encoded.len() as u64;
        assert_eq!(record(&mut encoded.as_slice(), length).unwrap(), message);
        let mut impossible_header = encoded.clone();
        impossible_header[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(record(&mut impossible_header.as_slice(), length).is_err());
        assert!(record(&mut encoded.as_slice(), length + 1).is_err());
        let mut invalid_text = encoded.clone();
        let last_text = invalid_text.len() - 33;
        invalid_text[last_text] = 0xff;
        assert!(record(&mut invalid_text.as_slice(), length).is_err());
        let mut changed_text = encoded.clone();
        changed_text[last_text] = b'x';
        assert!(
            record(&mut changed_text.as_slice(), length).is_err(),
            "valid UTF-8 changes must fail the checksum"
        );
        let mut changed_checksum = encoded.clone();
        *changed_checksum.last_mut().unwrap() ^= 1;
        assert!(record(&mut changed_checksum.as_slice(), length).is_err());
        for content_bytes in [u64::MAX, 1] {
            let header = Header {
                attachments: Vec::new(),
                id: "id".to_owned(),
                ordinal: 0,
                context: MessageContext::Root,
                role: Role::User,
                created_at: None,
                content_bytes,
                projection_bytes: Some(1),
                tools: crate::tools::Activity::default(),
            };
            let header = serde_json::to_vec(&header).unwrap();
            let mut encoded = Vec::new();
            encoded.extend_from_slice(&(header.len() as u64).to_le_bytes());
            encoded.extend_from_slice(&header);
            encoded.extend_from_slice(b"hello");
            encoded.extend_from_slice(&[0; 32]);
            assert!(record(&mut encoded.as_slice(), encoded.len() as u64).is_err());
        }
    }
}
