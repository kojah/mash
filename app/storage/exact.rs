//! Read-only lexical artifact access, independent of dense model readiness.
use super::{SearchHit, Storage, generated};
use crate::{AppError, gold::exact::Literal, search_filter::Scope};
use rusqlite::functions::FunctionFlags;

impl Storage {
    pub(crate) fn open_lexical(path: &std::path::Path) -> Result<Self, AppError> {
        let storage = Self::open_catalog(path)?;
        let silver = storage.silver_certificate()?;
        let certified = storage.gold_manifest()?.is_some_and(|m| {
            // This checks completed indexing and silver identity, not the
            // configured dense recipe or availability of its model files.
            m.matches(&m.build, &m.model_generation, &silver)
        });
        if storage.derived_search_is_dirty()? || !certified {
            return Err(AppError::search_not_ready(
                "lexical index is incomplete or stale; run `mash index`",
            ));
        }
        Ok(storage)
    }

    pub(crate) fn search_exact(
        &self,
        literal: &Literal,
        limit: usize,
        scope: &Scope,
    ) -> Result<Vec<SearchHit>, AppError> {
        let matcher = literal.clone();
        self.connection
            .create_scalar_function(
                "mash_literal",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                move |ctx| {
                    let content = ctx.get_raw(0).as_str()?;
                    Ok(matcher.matches(content))
                },
            )
            .map_err(AppError::database)?;
        let (projection, _) = generated::SearchHit::QUERY
            .split_once("FROM messages m")
            .ok_or_else(|| AppError::internal("invalid generated search projection"))?;
        let predicate = scope.filter.sql(Some(&scope.node));
        let mut statement = self
            .connection
            .prepare(&format!(
                "{projection}, bm25(message_fts) AS lexical_score
             FROM message_fts
             JOIN messages m ON m.id=message_fts.message_id
             JOIN conversations c ON c.id=m.conversation_id
             LEFT JOIN conversation_enrichment e ON e.conversation_id=c.id
             WHERE message_fts MATCH ?1 AND ({predicate})
               AND mash_literal(COALESCE(m.search_projection,m.content))
             ORDER BY lexical_score,m.id LIMIT ?2"
            ))
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(
                rusqlite::params![
                    literal.fts_query(),
                    i64::try_from(limit.min(1000)).unwrap_or(1000)
                ],
                |row| {
                    Ok((
                        generated::SearchHitRow::from_row(row)?,
                        row.get::<_, f64>("lexical_score")?,
                    ))
                },
            )
            .map_err(AppError::database)?;
        rows.map(|row| {
            let (row, lexical_score) = row.map_err(AppError::database)?;
            let mut hit = super::queries::search_hit(row)?;
            hit.lexical_score = Some(lexical_score);
            Ok(hit)
        })
        .collect()
    }
}
