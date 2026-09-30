//! Inspect JSON without building containers before decoding a typed record.
use crate::AppError;
use serde::{
    Deserializer,
    de::{DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};

const MAX_NODES: usize = 65_536;

pub(in crate::ingestion) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, AppError> {
    if bytes.len() > super::super::MAX_RECORD_BYTES {
        return Err(AppError::configuration(
            "provider JSON exceeds the 8 MiB ingestion limit",
        ));
    }
    validate(bytes, MAX_NODES).map_err(|_| {
        AppError::configuration("invalid provider JSON or structural ingestion limit exceeded")
    })?;
    serde_json::from_slice(bytes)
        .map_err(|_| AppError::configuration("invalid provider JSON schema"))
}

fn validate(bytes: &[u8], limit: usize) -> Result<(), serde_json::Error> {
    let mut remaining = limit;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    Seed(&mut remaining).deserialize(&mut deserializer)?;
    deserializer.end()
}

struct Seed<'a>(&'a mut usize);

impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        *self.0 = self
            .0
            .checked_sub(1)
            .ok_or_else(|| serde::de::Error::custom("provider JSON node limit exceeded"))?;
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed<'_> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a bounded JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        while sequence.next_element_seed(Seed(&mut *self.0))?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while map.next_key_seed(Seed(&mut *self.0))?.is_some() {
            map.next_value_seed(Seed(&mut *self.0))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_budget_counts_containers_keys_and_values() {
        let input = br#"{"a":[1,true,null,"escaped\ntext"],"b":{}}"#;
        assert!(validate(input, 9).is_ok());
        assert!(validate(input, 8).is_err());
        assert!(validate(b"[]", 1).is_ok());
        assert!(validate(b"[]", 0).is_err());
        assert!(validate(b"[] []", 10).is_err());
        assert!(validate(b"{\"a\":[1,", 10).is_err());
    }

    #[test]
    fn rejects_structural_amplification_and_deep_nesting_before_typed_decode() {
        let array = format!("[{}null]", "null,".repeat(MAX_NODES));
        assert!(decode::<Vec<Option<bool>>>(array.as_bytes()).is_err());
        let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        assert!(validate(nested.as_bytes(), MAX_NODES).is_err());
        assert_eq!(decode::<Vec<u64>>(b"[1,2]").unwrap(), [1, 2]);
    }
}
