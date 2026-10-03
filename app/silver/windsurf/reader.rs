//! Authenticate before inspecting or allocating typed native messages.
use super::{invalid, proto::Trajectory};
use crate::AppError;
use prost::{
    Message,
    encoding::{DecodeContext, WireType, decode_key, decode_varint, skip_field},
};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use std::{fs::File, io::Read, path::Path};

// This is the native format constant from the pinned public language server,
// not a mash secret or a credential read from the user's provider settings.
const KEY: &[u8; 32] = b"safeCodeiumworldKeYsecretBalloon";
const MAX_FILE: usize = 8 * 1024 * 1024;

pub(super) fn read(path: &Path) -> Result<Trajectory, AppError> {
    let file = File::open(path).map_err(AppError::io)?;
    if file.metadata().map_err(AppError::io)?.len() > MAX_FILE as u64 {
        return Err(invalid("native history exceeds the 8 MiB input limit"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(AppError::io)?;
    if bytes.len() > MAX_FILE {
        return Err(invalid("native history exceeds the 8 MiB input limit"));
    }
    decode(&mut bytes)
}

pub(super) fn decode(bytes: &mut [u8]) -> Result<Trajectory, AppError> {
    if bytes.len() < 28 || bytes.len() > MAX_FILE {
        return Err(invalid("invalid or oversized encrypted native history"));
    }
    let (nonce, body) = bytes.split_at_mut(12);
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| invalid("invalid nonce"))?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, KEY).map_err(|_| invalid("invalid native key"))?,
    );
    let plaintext = key
        .open_in_place(Nonce::assume_unique_for_key(nonce), Aad::empty(), body)
        .map_err(|_| invalid("native history authentication failed"))?;
    inspect(plaintext, Shape::Trajectory, &mut 65_536)?;
    Trajectory::decode(&*plaintext).map_err(|_| invalid("invalid native protobuf history"))
}

#[derive(Clone, Copy)]
enum Shape {
    Trajectory,
    Step,
    TrajectoryMetadata,
    StepMetadata,
    Leaf,
}

impl Shape {
    fn nested(self, tag: u32) -> Option<Self> {
        match (self, tag) {
            (Self::Trajectory, 2) => Some(Self::Step),
            (Self::Trajectory, 7) => Some(Self::TrajectoryMetadata),
            (Self::Step, 5) => Some(Self::StepMetadata),
            (Self::Step, 19 | 20)
            | (Self::TrajectoryMetadata, 1 | 2)
            | (Self::StepMetadata, 1 | 20) => Some(Self::Leaf),
            _ => None,
        }
    }
}

fn inspect(mut bytes: &[u8], shape: Shape, remaining: &mut usize) -> Result<(), AppError> {
    while !bytes.is_empty() {
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| invalid("native protobuf field limit exceeded"))?;
        let (tag, wire) = decode_key(&mut bytes).map_err(|_| invalid("invalid protobuf key"))?;
        if let Some(nested) = shape.nested(tag) {
            if wire != WireType::LengthDelimited {
                return Err(invalid("invalid native message wire type"));
            }
            let size = usize::try_from(
                decode_varint(&mut bytes).map_err(|_| invalid("invalid protobuf length"))?,
            )
            .map_err(|_| invalid("oversized protobuf length"))?;
            let message = bytes
                .get(..size)
                .ok_or_else(|| invalid("truncated native protobuf message"))?;
            inspect(message, nested, remaining)?;
            bytes = &bytes[size..];
        } else {
            skip_field(wire, tag, &mut bytes, DecodeContext::default())
                .map_err(|_| invalid("invalid protobuf field"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn seal(plaintext: &[u8]) -> Vec<u8> {
    let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, KEY).unwrap());
    let nonce = [0; 12];
    let mut body = plaintext.to_vec();
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut body)
        .unwrap();
    [nonce.as_slice(), &body].concat()
}
