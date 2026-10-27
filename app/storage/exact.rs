//! Read-only lexical artifact access, independent of dense model readiness.
use super::{SearchHit, Storage, generated};
use crate::{AppError, search_filter::Scope, views::exact::Literal};
use rusqlite::functions::FunctionFlags;

impl Storage {
    pub(crate) fn lexical_plan(
        &self,
        query: &str,
        mode: crate::views::matching::Mode,
        scope: Option<&Scope>,
    ) -> Result<crate::views::matching::Plan, AppError> {
        use crate::views::matching::{Mode, Plan, VOCABULARY_LIMIT};
        if mode == Mode::Strict || (query.trim().starts_with('"') && query.trim().ends_with('"')) {
            return Plan::new(query, Mode::Strict, &[]);
        }
        self.connection.execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS temp.mash_vocabulary USING fts5vocab(views,message_fts,row);
            CREATE VIRTUAL TABLE IF NOT EXISTS temp.mash_vocabulary_instances USING fts5vocab(views,message_fts,instance);").map_err(AppError::database)?;
        let unfiltered = scope.is_none_or(
            |s| matches!(&s.filter,crate::search_filter::Filter::All(parts) if parts.is_empty()),
        );
        let sql = if unfiltered {
            "SELECT term FROM temp.mash_vocabulary LIMIT ?1".to_owned()
        } else {
            let scope = scope.expect("filtered scope");
            // Resolve metadata once per indexed message, not once per token
            // occurrence. Keep the FTS scan outside the indexed object lookups:
            // metadata-first joins can rescan the virtual table for each message.
            format!(
                "SELECT DISTINCT v.term FROM temp.mash_vocabulary_instances v WHERE v.doc IN (
                SELECT f.rowid FROM message_fts f
                CROSS JOIN objects m INDEXED BY objects_id ON m.origin='provider' AND m.object_role='entry' AND m.id=f.message_id
                CROSS JOIN objects c INDEXED BY objects_native_container ON c.origin='provider' AND c.object_role='container' AND c.id=m.conversation_id
                WHERE ({})) LIMIT ?1",
                scope.filter.sql(Some(&scope.node))
            )
        };
        let vocabulary = self
            .connection
            .prepare(&sql)
            .map_err(AppError::database)?
            .query_map(
                [i64::try_from(VOCABULARY_LIMIT + 1).expect("bounded vocabulary")],
                |row| row.get::<_, String>(0),
            )
            .map_err(AppError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)?;
        if vocabulary.len() > VOCABULARY_LIMIT {
            return Err(AppError::usage(
                "lexical matching vocabulary budget exceeded; narrow filters or use match_mode=strict",
            ));
        }
        Plan::new(query, mode, &vocabulary)
    }
    pub(crate) fn open_lexical(path: &std::path::Path) -> Result<Self, AppError> {
        let storage = Self::open_catalog(path)?;
        let silver = storage.silver_certificate()?;
        let searchable_messages = storage.counts()?.searchable_messages;
        let certified = storage
            .lexical_manifest()?
            .is_some_and(|m| m.matches(&silver, searchable_messages));
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
        let plan = self.lexical_plan(literal.text(), literal.mode(), Some(scope))?;
        let fts = if literal.mode() == crate::views::matching::Mode::Strict {
            literal.fts_query()
        } else {
            plan.sqlite()
        };
        self.connection
            .create_scalar_function(
                "mash_literal",
                2,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                move |ctx| {
                    // Only native messages carry a role and match; SQLite may
                    // call this before the join has excluded other objects.
                    let Some(role) = ctx.get::<Option<crate::role::Role>>(0)? else {
                        return Ok(0);
                    };
                    let Ok(content) = ctx.get_raw(1).as_str() else {
                        return Ok(0);
                    };
                    let visible = crate::silver::dialogue::visible(&role, content);
                    Ok(matcher
                        .evidence(&visible)
                        .map_or(0, |evidence| match evidence.kind {
                            crate::views::matching::Kind::Exact => 4,
                            crate::views::matching::Kind::Prefix => 3,
                            crate::views::matching::Kind::Typo => 2,
                            crate::views::matching::Kind::PrefixAndTypo => 1,
                        }))
                },
            )
            .map_err(AppError::database)?;
        let (projection, _) = generated::SearchHit::QUERY
            .split_once("FROM objects m")
            .ok_or_else(|| AppError::internal("invalid generated search projection"))?;
        let predicate = scope.filter.sql(Some(&scope.node));
        let mut statement = self
            .connection
            .prepare(&format!(
                "{projection}, 0.0 AS lexical_score
             FROM message_fts
             JOIN objects m ON m.origin='provider' AND m.object_role='entry' AND m.id=message_fts.message_id
             JOIN objects c ON c.origin='provider' AND c.object_role='container' AND c.id=m.conversation_id
             LEFT JOIN conversation_enrichment e ON e.conversation_id=c.id
             WHERE message_fts MATCH ?1 AND ({predicate})
               AND mash_literal(m.role,COALESCE(m.search_projection,m.content))
             ORDER BY mash_literal(m.role,COALESCE(m.search_projection,m.content)) DESC,
                      m.created_at DESC,m.id LIMIT ?2"
            ))
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(
                rusqlite::params![fts, i64::try_from(limit.min(1000)).unwrap_or(1000)],
                |row| {
                    Ok((
                        generated::SearchHitRow::from_row(row)?,
                        row.get::<_, f64>("lexical_score")?,
                    ))
                },
            )
            .map_err(AppError::database)?;
        let mut hits = rows
            .map(|row| {
                let (row, lexical_score) = row.map_err(AppError::database)?;
                let mut hit = super::queries::search_hit(&self.connection, row)?;
                hit.lexical_score = Some(lexical_score);
                crate::views::matching::annotate(&mut hit, literal, self.output_redaction);
                Ok(hit)
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let limit = limit.min(1000);
        let ranked = self.search_connector_lexical(&fts, limit, None, Some(scope))?;
        let ids = ranked.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        for mut hit in self.connector_hits(&ids)? {
            crate::views::matching::annotate(&mut hit, literal, self.output_redaction);
            if hit.lexical_match.is_some() {
                hit.lexical_score = Some(0.0);
                hits.push(hit);
            }
        }
        // The native query's own order: match quality, then recency, then ID.
        hits.sort_by(|left, right| {
            quality(right)
                .cmp(&quality(left))
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

fn quality(hit: &SearchHit) -> u8 {
    hit.lexical_match
        .as_ref()
        .map_or(0, |evidence| match evidence.kind {
            crate::views::matching::Kind::Exact => 4,
            crate::views::matching::Kind::Prefix => 3,
            crate::views::matching::Kind::Typo => 2,
            crate::views::matching::Kind::PrefixAndTypo => 1,
        })
}
