//! Shared native timestamp validation and unambiguous workspace URI projection.
use crate::AppError;
use prost::Message;
use std::collections::BTreeSet;

#[derive(Clone, Message)]
pub(super) struct Timestamp {
    #[prost(int64, tag = "1")]
    pub seconds: i64,
    #[prost(int32, tag = "2")]
    pub nanos: i32,
}
#[derive(Clone, Message)]
pub(super) struct Workspace {
    #[prost(string, tag = "1")]
    pub project: String,
    #[prost(string, tag = "2")]
    pub repository: String,
}
pub(super) fn timestamp(value: Option<&Timestamp>) -> Result<Option<i64>, AppError> {
    value
        .map(|t| {
            if !(0..=253_402_300_799).contains(&t.seconds) || !(0..1_000_000_000).contains(&t.nanos)
            {
                return Err(AppError::configuration("invalid native timestamp"));
            }
            Ok(t.seconds * 1000 + i64::from(t.nanos) / 1_000_000)
        })
        .transpose()
}
pub(super) fn location<'a>(uris: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut paths = BTreeSet::new();
    for uri in uris.filter(|u| !u.is_empty()) {
        let path = reqwest::Url::parse(uri).ok()?.to_file_path().ok()?;
        paths.insert(path.to_str()?.to_owned());
    }
    if paths.len() == 1 {
        paths.pop_first()
    } else {
        None
    }
}
