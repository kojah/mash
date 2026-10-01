//! Opaque per-hop correlation; incoming metadata never becomes arbitrary log text.
pub(crate) const HEADER: &str = "x-mash-trace-id";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct TraceId(String);

impl TryFrom<String> for TraceId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value.as_bytes()).ok_or("invalid trace ID")
    }
}

impl From<TraceId> for String {
    fn from(value: TraceId) -> Self {
        value.0
    }
}

impl TraceId {
    pub(crate) fn parse(value: &[u8]) -> Option<Self> {
        if value.len() != 32
            || !value
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return None;
        }
        String::from_utf8(value.to_vec()).ok().map(Self)
    }

    pub(crate) fn generate() -> Option<Self> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).ok()?;
        let mut value = String::with_capacity(32);
        for byte in bytes {
            value.push(HEX[usize::from(byte >> 4)] as char);
            value.push(HEX[usize::from(byte & 15)] as char);
        }
        Some(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bounded_lowercase_hex_is_accepted() {
        let valid = "0123456789abcdef0123456789abcdef";
        assert_eq!(TraceId::parse(valid.as_bytes()).unwrap().as_str(), valid);
        for invalid in [
            "",
            "abc",
            "0123456789ABCDEF0123456789ABCDEF",
            "secret-canary",
            "0000000000000000000000000000000\0",
            "fffffffffffffffffffffffffffffffff",
        ] {
            assert!(TraceId::parse(invalid.as_bytes()).is_none());
        }
        assert!(TraceId::parse(&[255; 32]).is_none());
        let generated = TraceId::generate().unwrap();
        assert!(TraceId::parse(generated.as_str().as_bytes()).is_some());
    }
}
