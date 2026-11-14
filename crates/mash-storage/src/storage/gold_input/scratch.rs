//! Private, disposable replay frames. Not an archive or a publication format.
use crate::{AppError, message_context::MessageContext, views::documents::SearchMessage};
use prost::Message;
use std::io::Read;

#[derive(Message)]
struct Row {
    #[prost(string, tag = "1")]
    id: String,
    #[prost(sint64, tag = "2")]
    ordinal: i64,
    // Same ancestry convention as silver: absent = linear, empty = root.
    #[prost(string, optional, tag = "3")]
    parent: Option<String>,
    #[prost(string, tag = "4")]
    role: String,
    #[prost(string, tag = "5")]
    projection: String,
    #[prost(sint64, optional, tag = "6")]
    created_at: Option<i64>,
}

pub(super) fn frame(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let length = u32::try_from(bytes.len()).map_err(|_| invalid())?;
    let mut framed = Vec::with_capacity(bytes.len() + 4);
    framed.extend_from_slice(&length.to_le_bytes());
    framed.extend_from_slice(bytes);
    Ok(framed)
}

pub(super) fn encode(message: SearchMessage) -> Result<Vec<u8>, AppError> {
    let parent = match message.context {
        MessageContext::Linear => None,
        MessageContext::Root => Some(String::new()),
        MessageContext::Parent { id } => Some(id),
    };
    frame(
        &Row {
            id: message.id,
            ordinal: message.ordinal,
            parent,
            role: message.role.into(),
            projection: message.projection,
            created_at: message.created_at,
        }
        .encode_to_vec(),
    )
}

pub(super) fn read(reader: &mut impl Read, remaining: &mut u64) -> Result<Vec<u8>, AppError> {
    if *remaining < 4 {
        return Err(invalid());
    }
    let mut prefix = [0; 4];
    reader.read_exact(&mut prefix).map_err(AppError::io)?;
    let length = u32::from_le_bytes(prefix);
    let consumed = u64::from(length) + 4;
    if consumed > *remaining {
        return Err(invalid());
    }
    let mut bytes = vec![0; usize::try_from(length).map_err(|_| invalid())?];
    reader.read_exact(&mut bytes).map_err(AppError::io)?;
    *remaining -= consumed;
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8], chat: &str) -> Result<SearchMessage, AppError> {
    let row = Row::decode(bytes).map_err(|_| invalid())?;
    if row.id.is_empty() {
        return Err(invalid());
    }
    Ok(SearchMessage {
        id: row.id,
        chat_id: chat.into(),
        ordinal: row.ordinal,
        context: MessageContext::from_parent(row.parent),
        role: row.role.into(),
        projection: row.projection,
        created_at: row.created_at,
        generation: None,
        vector: None,
    })
}

fn invalid() -> AppError {
    AppError::internal("invalid replay scratch frame")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn scratch_roundtrip_preserves_text_roles_ancestry_and_optional_times(
            id in "(?s:.{1,30})", text in "(?s:.{0,200})", role in "(?s:.{0,20})",
            ordinal in any::<i64>(), created_at in prop::option::of(any::<i64>()),
            parent in prop::option::of(".{0,30}"),
        ) {
            let message = SearchMessage {
                id, chat_id: "chat".into(), ordinal,
                context: MessageContext::from_parent(parent), role: role.into(),
                projection: text, created_at, generation: None, vector: None,
            };
            let expected = serde_json::to_value(&message).unwrap();
            let bytes = encode(message).unwrap();
            let mut remaining = bytes.len() as u64;
            let body = read(&mut bytes.as_slice(), &mut remaining).unwrap();
            prop_assert_eq!(remaining, 0);
            prop_assert_eq!(serde_json::to_value(decode(&body, "chat").unwrap()).unwrap(), expected);
        }
    }

    #[test]
    fn scratch_rejects_truncation_and_lengths_outside_chat_before_allocating() {
        for bytes in [
            &[][..],
            &[1, 0, 0][..],
            &[255, 255, 255, 255][..],
            &[2, 0, 0, 0, 1][..],
        ] {
            assert!(read(&mut &*bytes, &mut (bytes.len() as u64)).is_err());
        }
        assert!(decode(&[255], "chat").is_err());
        assert!(decode(&[], "chat").is_err());
        let framed = frame(b"complete body beyond this chat").unwrap();
        for mut budget in 0..4 {
            let mut reader = std::io::Cursor::new(&framed);
            assert!(read(&mut reader, &mut budget).is_err());
            assert_eq!(reader.position(), 0);
        }
        let mut reader = std::io::Cursor::new(&framed);
        assert!(read(&mut reader, &mut 5).is_err());
        assert_eq!(reader.position(), 4);
        assert_eq!(
            read(&mut frame(b"").unwrap().as_slice(), &mut 4).unwrap(),
            b""
        );
    }
}
