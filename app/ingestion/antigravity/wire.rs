//! Bound native protobuf structure before prost allocates repeated fields.
use super::{invalid, proto};
use crate::AppError;
use prost::{
    Message,
    encoding::{DecodeContext, WireType, decode_key, decode_varint, skip_field},
};

#[derive(Clone, Copy)]
pub(super) enum Shape {
    Step,
    Metadata,
    StepMetadata,
    User,
    Item,
    Scope,
    Leaf,
}
impl Shape {
    fn nested(self, tag: u32) -> Option<Self> {
        match (self, tag) {
            (Self::Step, 5) => Some(Self::StepMetadata),
            (Self::Step, 19) => Some(Self::User),
            (Self::Step, 20)
            | (Self::StepMetadata, 1 | 20)
            | (Self::Metadata, 1 | 2 | 8)
            | (Self::Scope, 11) => Some(Self::Leaf),
            (Self::User, 3) => Some(Self::Item),
            (Self::Item, 2) => Some(Self::Scope),
            _ => None,
        }
    }
}
pub(super) fn inspect(
    mut bytes: &[u8],
    shape: Shape,
    remaining: &mut usize,
) -> Result<(), AppError> {
    while !bytes.is_empty() {
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| invalid("protobuf field limit exceeded"))?;
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
                .ok_or_else(|| invalid("truncated protobuf message"))?;
            inspect(message, nested, remaining)?;
            bytes = &bytes[size..];
        } else {
            skip_field(wire, tag, &mut bytes, DecodeContext::default())
                .map_err(|_| invalid("invalid protobuf field"))?;
        }
    }
    Ok(())
}
pub(super) fn step(
    bytes: &[u8],
    metadata: Option<&[u8]>,
    remaining: &mut usize,
) -> Result<proto::Step, AppError> {
    inspect(bytes, Shape::Step, remaining)?;
    let mut fields = bytes;
    let mut found = None;
    let mut singles = std::collections::BTreeSet::new();
    while !fields.is_empty() {
        let (tag, wire) = decode_key(&mut fields).map_err(|_| invalid("invalid protobuf key"))?;
        if matches!(tag, 1 | 4 | 5 | 19 | 20) && !singles.insert(tag) {
            return Err(invalid("duplicate native step field"));
        }
        if tag == 5 {
            let size = usize::try_from(
                decode_varint(&mut fields).map_err(|_| invalid("invalid metadata length"))?,
            )
            .map_err(|_| invalid("invalid metadata length"))?;
            found = Some(
                fields
                    .get(..size)
                    .ok_or_else(|| invalid("invalid metadata length"))?,
            );
            fields = &fields[size..];
        } else {
            skip_field(wire, tag, &mut fields, DecodeContext::default())
                .map_err(|_| invalid("invalid protobuf field"))?;
        }
    }
    if found != metadata {
        return Err(invalid("SQL and protobuf metadata disagree"));
    }
    proto::Step::decode(bytes).map_err(|_| invalid("invalid native step protobuf"))
}
