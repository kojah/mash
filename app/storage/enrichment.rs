//! Canonical metadata evidence, retained independently of gold builds.
use super::{Storage, generated};
use crate::{AppError, provider::Provider, silver::enrichment::Evidence};

pub(super) fn decode(json: &str) -> Result<Evidence, AppError> {
    serde_json::from_str(json)
        .map_err(|_| AppError::database_data("invalid silver enrichment evidence"))
}

impl Storage {
    pub(super) fn store_enrichment(&self, id: &str, evidence: &Evidence) -> Result<(), AppError> {
        let json = serde_json::to_string(evidence)
            .map_err(|_| AppError::internal("serialize silver enrichment evidence"))?;
        self.connection
            .execute(
                "INSERT INTO conversation_enrichment(conversation_id,evidence) VALUES(?1,?2)
             ON CONFLICT(conversation_id) DO UPDATE SET evidence=excluded.evidence
             WHERE evidence IS NOT excluded.evidence",
                rusqlite::params![id, json],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(super) fn source_enrichment_is_current(
        &self,
        provider: Provider,
        key: &str,
    ) -> Result<bool, AppError> {
        let rows = generated::SourceEnrichment::builder()
            .provider(provider.as_str())
            .source_key(key)
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?;
        for row in rows {
            let Some(json) = row.evidence else {
                return Ok(false);
            };
            let evidence = decode(&json)?;
            if !evidence.is_current_for(provider)
                || evidence
                    .timestamps
                    .as_ref()
                    .is_some_and(|times| !times.matches_values(row.created_at, row.updated_at))
            {
                return Ok(false);
            }
        }
        // Empty/tombstoned source contributions have no metadata to enrich.
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn conversation_enrichment(&self, id: &str) -> Result<Option<Evidence>, AppError> {
        generated::ConversationEnrichment::builder()
            .conversation_id(id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .map(|row| decode(&row.evidence))
            .transpose()
    }
}

#[cfg(test)]
mod tests;
