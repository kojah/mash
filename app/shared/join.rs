//! A join string: everything a new machine needs to publish to and search a
//! shared index, minted by the server for one node and pasted into
//! `mash init --join`. It holds that node's token, so it is a secret.
use crate::AppError;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "mash-join.";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Join {
    /// Format version; a newer server's string is refused, not guessed at.
    v: u8,
    pub(crate) api_url: String,
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) token: String,
}

impl Join {
    pub(crate) fn new(api_url: &str, group_id: &str, node_id: &str, token: &str) -> Self {
        Self {
            v: 1,
            api_url: api_url.trim_end_matches('/').to_owned(),
            group_id: group_id.to_owned(),
            node_id: node_id.to_owned(),
            token: token.to_owned(),
        }
    }

    pub(crate) fn encode(&self) -> Result<String, AppError> {
        let json =
            serde_json::to_vec(self).map_err(|_| AppError::internal("encode join string"))?;
        Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(json)))
    }

    /// Parse and validate a pasted string. Errors never echo it: it is a secret.
    pub fn decode(text: &str) -> Result<Self, AppError> {
        let invalid = || {
            AppError::usage(
                "that is not a Mash join string; copy the whole `mash-join.…` value the server printed",
            )
        };
        let encoded = text.trim().strip_prefix(PREFIX).ok_or_else(invalid)?;
        let json = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| invalid())?;
        let join: Self = serde_json::from_slice(&json).map_err(|_| invalid())?;
        if join.v != 1 {
            return Err(AppError::usage(
                "this join string was made by a newer Mash; upgrade this machine first",
            ));
        }
        crate::config::validate_identifier(&join.group_id, "group_id").map_err(|_| invalid())?;
        crate::config::validate_identifier(&join.node_id, "node_id").map_err(|_| invalid())?;
        let url = reqwest::Url::parse(&join.api_url).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
            return Err(invalid());
        }
        if !(32..=4096).contains(&join.token.len())
            || !join.token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(invalid());
        }
        Ok(join)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_join_string_round_trips_and_trims_the_address() {
        let join = Join::new("https://mash.example.net/", "personal", "laptop", TOKEN);
        let text = join.encode().unwrap();
        assert!(text.starts_with("mash-join."));
        assert!(
            !text.contains(TOKEN),
            "the token is not readable at a glance"
        );
        let decoded = Join::decode(&format!("  {text}\n")).unwrap();
        assert_eq!(decoded, join);
        assert_eq!(decoded.api_url, "https://mash.example.net");
    }

    #[test]
    fn anything_else_is_refused_without_echoing_it() {
        let encode = |value: serde_json::Value| {
            format!("mash-join.{}", URL_SAFE_NO_PAD.encode(value.to_string()))
        };
        let good = serde_json::json!({"v":1,"api_url":"https://h","group_id":"g","node_id":"n","token":TOKEN});
        assert!(Join::decode(&encode(good.clone())).is_ok());
        let mut cases = vec![
            String::new(),
            "mash-join.".to_owned(),
            "mash-join.not base64!".to_owned(),
            encode(serde_json::json!({"v":1})),
        ];
        for (field, value) in [
            ("api_url", serde_json::json!("ftp://h")),
            ("api_url", serde_json::json!("not a url")),
            ("group_id", serde_json::json!("has space")),
            ("node_id", serde_json::json!("")),
            ("token", serde_json::json!("short")),
            ("token", serde_json::json!(format!("{TOKEN} with space"))),
            ("v", serde_json::json!(2)),
            ("extra", serde_json::json!(true)),
        ] {
            let mut changed = good.clone();
            changed[field] = value;
            cases.push(encode(changed));
        }
        for case in cases {
            let error = Join::decode(&case).unwrap_err();
            assert_eq!(error.error.kind, crate::ErrorKind::Usage, "{case}");
            assert!(!error.error.message.contains(TOKEN));
        }
    }
}
