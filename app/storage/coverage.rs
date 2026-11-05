//! Read canonical retained rows and recorded provider scan evidence, without views.
use super::Storage;
use crate::{
    AppError,
    core::coverage::Retained,
    provider::Provider,
    silver::certificate::{Completeness, INTERPRETATION_VERSION},
};
use rusqlite::OptionalExtension;

fn count(row: &rusqlite::Row<'_>, column: &str) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|_| {
        rusqlite::Error::IntegralValueOutOfRange(
            row.as_ref().column_index(column).unwrap_or(0),
            value,
        )
    })
}

impl Storage {
    pub(crate) fn coverage_scan(
        &self,
        provider: Provider,
    ) -> Result<(Completeness, u64), AppError> {
        let scan = self
            .connection
            .query_row(
                "SELECT interpretation_version,state FROM silver_scans WHERE provider=?1",
                [provider],
                |row| {
                    Ok((
                        row.get::<_, u32>("interpretation_version")?,
                        row.get::<_, i64>("state")?,
                    ))
                },
            )
            .optional()
            .map_err(AppError::database)?;
        let rejected = self
            .connection
            .query_row(
                "SELECT count(*) AS rejected FROM silver_rejections WHERE provider=?1",
                [provider],
                |row| count(row, "rejected"),
            )
            .map_err(AppError::database)?;
        let state = match scan {
            None => Completeness::Unverified,
            Some((version, _)) if version != INTERPRETATION_VERSION => Completeness::Unverified,
            Some((_, 0)) => Completeness::Pending,
            Some((_, 1)) if rejected == 0 => Completeness::Complete,
            Some((_, 1 | 2)) => Completeness::Incomplete,
            Some(_) => return Err(AppError::database_data("invalid provider scan state")),
        };
        Ok((state, rejected))
    }
    pub(crate) fn coverage_retained(
        &self,
        scope: &crate::search_filter::Scope,
    ) -> Result<Retained, AppError> {
        self.connection.query_row(&format!("SELECT count(DISTINCT c.id) AS chats,count(m.id) AS messages,min(m.created_at) AS earliest,max(m.created_at) AS latest FROM chats c LEFT JOIN messages m ON m.chat_id=c.id WHERE ({})",scope.filter.sql(Some(&scope.node))), [], |row| {
            Ok(Retained { chats: count(row, "chats")?, messages: count(row, "messages")?, earliest_message_at: row.get("earliest")?, latest_message_at: row.get("latest")? })
        }).map_err(AppError::database)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_scan_distinguishes_pending_rejected_and_outdated_interpretation() {
        let root = tempfile::tempdir().unwrap();
        let mut storage = Storage::open_writer(&root.path().join("silver.sqlite3")).unwrap();
        assert_eq!(
            storage.coverage_scan(Provider::Codex).unwrap(),
            (Completeness::Unverified, 0)
        );
        storage.begin_silver_scan(Provider::Codex).unwrap();
        assert_eq!(
            storage.coverage_scan(Provider::Codex).unwrap(),
            (Completeness::Pending, 0)
        );
        storage.finish_silver_scan(Provider::Codex, true).unwrap();
        assert_eq!(
            storage.coverage_scan(Provider::Codex).unwrap(),
            (Completeness::Complete, 0)
        );
        storage.connection.execute("INSERT INTO silver_rejections(provider,source_key,source_path,modified_ns) VALUES('codex','fixture','fixture',0)", []).unwrap();
        assert_eq!(
            storage.coverage_scan(Provider::Codex).unwrap(),
            (Completeness::Incomplete, 1)
        );
        storage
            .connection
            .execute(
                "UPDATE silver_scans SET interpretation_version=?1 WHERE provider='codex'",
                [INTERPRETATION_VERSION - 1],
            )
            .unwrap();
        assert_eq!(
            storage.coverage_scan(Provider::Codex).unwrap(),
            (Completeness::Unverified, 1)
        );
    }
}
