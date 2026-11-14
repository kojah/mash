//! SQLite failures retain their evidence; a lock code does not identify its owner.

use serde::{Deserialize, Serialize};

use crate::{AppError, ErrorKind};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "DatabaseOperation")]
pub enum Operation {
    Unspecified,
    OpenCatalog,
    InspectCatalog,
    InspectStatus,
    ConfigureWriter,
    BeginWriter,
}

impl Operation {
    fn description(self) -> &'static str {
        match self {
            Self::Unspecified => "database operation",
            Self::OpenCatalog => "open read-only history database",
            Self::InspectCatalog => "inspect history catalog",
            Self::InspectStatus => "inspect local index status",
            Self::ConfigureWriter => "configure database writer",
            Self::BeginWriter => "begin index writer transaction",
        }
    }
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DatabaseDetails {
    operation: Operation,
    #[serde(skip_serializing_if = "Option::is_none")]
    sqlite_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sqlite_extended_code: Option<i32>,
}

pub fn error(operation: Operation, error: rusqlite::Error) -> AppError {
    let sqlite = error.sqlite_error();
    let locked = sqlite.is_some_and(|details| {
        matches!(
            details.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
        )
    });
    let (exit_code, kind) = if locked {
        (6, ErrorKind::IndexBusy)
    } else {
        (5, ErrorKind::Database)
    };
    let message = format!("{}: {error}", operation.description());
    let mut result = AppError::new(exit_code, kind, message, true);
    result.error.database = Some(DatabaseDetails {
        operation,
        sqlite_code: sqlite.map(|details| details.extended_code & 0xff),
        sqlite_extended_code: sqlite.map(|details| details.extended_code),
    });
    drop(error);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_and_locked_keep_distinct_sqlite_evidence() {
        for (extended, reason) in [
            (517, "snapshot cannot be upgraded"),
            (262, "shared cache locked"),
        ] {
            let failure = rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(extended),
                Some(reason.into()),
            );
            let result = error(Operation::InspectStatus, failure);
            assert_eq!(result.code, 6);
            let response = serde_json::to_value(result).unwrap();
            assert_eq!(response["error"]["kind"], "index-busy");
            assert_eq!(response["error"]["retryable"], true);
            assert!(
                response["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(reason)
            );
            assert_eq!(response["error"]["database"]["operation"], "inspect-status");
            assert_eq!(
                response["error"]["database"]["sqlite_code"],
                extended & 0xff
            );
            assert_eq!(
                response["error"]["database"]["sqlite_extended_code"],
                extended
            );
        }
    }

    #[test]
    fn ordinary_database_failure_keeps_reason_and_operation() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        let failure = connection
            .execute_batch("SELECT * FROM missing_table")
            .unwrap_err();
        let result = error(Operation::InspectCatalog, failure);
        assert_eq!(result.code, 5);
        let response = serde_json::to_value(result).unwrap();
        assert_eq!(response["error"]["kind"], "database");
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("missing_table")
        );
        assert_eq!(
            response["error"]["database"]["operation"],
            "inspect-catalog"
        );
        assert_eq!(response["error"]["database"]["sqlite_code"], 1);
    }

    #[test]
    fn non_sqlite_and_unrelated_errors_do_not_invent_sqlite_codes() {
        let failure = error(Operation::Unspecified, rusqlite::Error::QueryReturnedNoRows);
        let response = serde_json::to_value(failure).unwrap();
        assert!(response["error"]["database"].get("sqlite_code").is_none());
        let unrelated = serde_json::to_value(AppError::usage("invalid request")).unwrap();
        assert!(unrelated["error"].get("database").is_none());
    }
}
