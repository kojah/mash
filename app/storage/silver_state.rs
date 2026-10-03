//! Durable silver evidence; independent of embeddings and other gold writes.
use super::{Storage, generated};
use crate::AppError;
use crate::provider::Provider;
use crate::silver::certificate::{Certificate, Completeness, INTERPRETATION_VERSION};
use crate::silver::{SourceIdentity, SourceStamp};
use rusqlite::params;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Copy)]
#[repr(i64)]
enum ScanState {
    Pending = 0,
    Complete = 1,
    Incomplete = 2,
}

impl Storage {
    pub(crate) fn silver_certificate(&self) -> Result<Certificate, AppError> {
        let row = generated::SilverCertificate::builder()
            .current_version(i64::from(INTERPRETATION_VERSION))
            .build()
            .query_one(&self.connection)
            .map_err(AppError::database)?;
        let completeness = if row.pending != 0 {
            Completeness::Pending
        } else if row.unverified != 0 {
            Completeness::Unverified
        } else if row.incomplete != 0 || row.rejected_sources != 0 {
            Completeness::Incomplete
        } else {
            Completeness::Complete
        };
        Ok(Certificate {
            revision: u64::try_from(row.revision)
                .map_err(|_| AppError::database_data("invalid silver revision"))?,
            interpretation_version: u32::try_from(row.interpretation_version)
                .map_err(|_| AppError::database_data("invalid silver interpretation version"))?,
            completeness,
            rejected_sources: u64::try_from(row.rejected_sources)
                .map_err(|_| AppError::database_data("invalid silver rejection count"))?,
        })
    }

    pub(crate) fn begin_silver_scan(&mut self, provider: Provider) -> Result<(), AppError> {
        self.require_writer()?;
        // Retain only the identity of the previously published gold snapshot.
        // This cannot authorize a new build; gold may resume it if silver and
        // model identity are unchanged after a rejected source contributes nothing.
        self.connection.execute(
            "UPDATE derived_state SET resume_generation=semantic_ready_generation,
                resume_silver_revision=(SELECT revision FROM silver_state WHERE singleton=1),
                resume_interpretation_version=(SELECT interpretation_version FROM silver_state WHERE singleton=1)
             WHERE singleton=1 AND semantic_ready_generation IS NOT NULL
               AND (SELECT interpretation_version FROM silver_state WHERE singleton=1)=?1",
            [INTERPRETATION_VERSION],
        ).map_err(AppError::database)?;
        self.connection.execute(
            "INSERT INTO silver_scans(provider,interpretation_version,state) VALUES(?1,?2,?3)
             ON CONFLICT(provider) DO UPDATE SET interpretation_version=excluded.interpretation_version,state=excluded.state",
            params![provider, INTERPRETATION_VERSION, ScanState::Pending as i64],
        ).map_err(AppError::database)?;
        // A checkpointed partial scan must not retain a published ready marker.
        self.mark_semantic_index_incomplete()
    }

    pub(crate) fn finish_silver_scan(
        &mut self,
        provider: Provider,
        complete: bool,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        let state = if complete {
            ScanState::Complete
        } else {
            ScanState::Incomplete
        };
        self.connection
            .execute(
                "UPDATE silver_scans SET state=?2 WHERE provider=?1",
                params![provider, state as i64],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn finish_silver_ingestion(&mut self) -> Result<(), AppError> {
        self.require_writer()?;
        self.connection
            .execute(
                "UPDATE silver_state SET interpretation_version=?1 WHERE singleton=1",
                [INTERPRETATION_VERSION],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn reject_silver_source(
        &mut self,
        provider: Provider,
        source: &SourceIdentity,
        stamp: &SourceStamp,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        self.connection.execute(
            "INSERT INTO silver_rejections(provider,source_key,source_path,modified_ns) VALUES(?1,?2,?3,?4)
             ON CONFLICT(provider,source_key) DO UPDATE SET source_path=excluded.source_path,modified_ns=excluded.modified_ns",
            params![provider, source.key(), source.path.to_string_lossy(), stamp.activity_ns.unwrap_or(stamp.modified_ns)],
        ).map_err(AppError::database)?;
        Ok(())
    }

    pub(super) fn clear_silver_rejection(
        &mut self,
        provider: Provider,
        key: &str,
    ) -> Result<(), AppError> {
        self.connection
            .execute(
                "DELETE FROM silver_rejections WHERE provider=?1 AND source_key=?2",
                params![provider, key],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(super) fn purge_missing_silver_rejections(
        &mut self,
        provider: Provider,
        observed: &BTreeSet<String>,
        roots: &[PathBuf],
        cutoff: Option<i64>,
    ) -> Result<(), AppError> {
        let records = generated::SilverRejections::builder()
            .provider(provider.as_str())
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?;
        for record in records {
            if !observed.contains(&record.source_key)
                && super::indexing::source_is_in_reconciliation_scope(
                    &record.source_path,
                    Some(record.modified_ns),
                    roots,
                    cutoff,
                )
            {
                self.clear_silver_rejection(provider, &record.source_key)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
