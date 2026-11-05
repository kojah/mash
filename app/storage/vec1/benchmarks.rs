//! Historical streaming control, excluded from production. All engines share
//! one corpus and IVF model. A metadata-streaming control separates predicate
//! placement from the benefit of disabling streaming once all filters push down.
use super::*;

impl Storage {
    pub(crate) fn vec1_canonical_eligible_ids(
        &self,
        generation: &str,
        scope: &crate::search_filter::Scope,
    ) -> Result<std::collections::BTreeSet<String>, AppError> {
        let queries = [
            format!("SELECT m.id FROM message_embeddings e JOIN objects m ON m.id=e.message_id AND m.origin='provider' AND m.object_role='entry'
                JOIN objects c ON c.id=m.conversation_id AND c.origin='provider' AND c.object_role='container'
                WHERE e.generation=?1 AND ({})", scope.filter.sql(Some(&scope.node))),
            format!("SELECT u.anchor FROM connector_embeddings e JOIN connector_units u ON u.id=e.unit_id
                WHERE e.generation=?1 AND ({})", scope.filter.connector_sql(Some(&scope.node))),
        ];
        let mut result = std::collections::BTreeSet::new();
        for sql in queries {
            let mut statement = self.connection.prepare(&sql).map_err(AppError::database)?;
            for row in statement
                .query_map([generation], |row| row.get::<_, String>(0))
                .map_err(AppError::database)?
            {
                if !result.insert(row.map_err(AppError::database)?) {
                    return Err(AppError::database_data("canonical eligibility IDs collide"));
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn vec1_projection_mismatches(&self) -> Result<i64, AppError> {
        self.connection.query_row("SELECT count(*) FROM vec1_index v
            LEFT JOIN vec1_points p ON p.id=v.rowid
            LEFT JOIN objects m ON p.connector=0 AND m.storage_id=p.target_id AND m.origin='provider' AND m.object_role='entry'
            LEFT JOIN objects c ON c.id=m.conversation_id AND c.origin='provider' AND c.object_role='container'
            LEFT JOIN connector_units u ON p.connector=1 AND u.id=p.target_id
            WHERE p.id IS NULL OR (p.connector=0 AND (m.storage_id IS NULL OR c.storage_id IS NULL)) OR (p.connector=1 AND u.id IS NULL)
            OR v.connector IS NOT p.connector OR v.point_key IS NOT p.id
            OR v.created_at IS NOT CASE WHEN p.connector=0 THEN m.created_at ELSE u.created_at END
            OR v.provider IS NOT CASE WHEN p.connector=0 THEN c.source ELSE NULL END
            OR v.role IS NOT CASE WHEN p.connector=0 THEN m.role ELSE NULL END
            OR v.chat IS NOT CASE WHEN p.connector=0 THEN m.conversation_id ELSE u.group_anchor END
            OR v.project IS NOT CASE WHEN p.connector=0 THEN c.project ELSE NULL END
            OR v.repository IS NOT CASE WHEN p.connector=0 THEN c.repository ELSE NULL END
            OR v.source IS NOT CASE WHEN p.connector=1 THEN u.source_id ELSE NULL END
            OR v.kind IS NOT CASE WHEN p.connector=0 THEN m.kind ELSE u.label END", [], |row|row.get(0)).map_err(AppError::database)
    }

    pub(crate) fn vec1_metadata_streaming_control(
        &self,
        generation: &str,
        query: &[u8],
        filters: &crate::search_filter::Scope,
        limit: usize,
    ) -> Result<(Vec<Candidate>, usize, Timings), AppError> {
        self.vec1_profiled(generation, query, (None, None, Some(filters)), limit, true)
    }

    pub(crate) fn vec1_streaming_control(
        &self,
        generation: &str,
        query: &[u8],
        filters: &crate::search_filter::Scope,
        limit: usize,
    ) -> Result<(Vec<Candidate>, usize, Timings), AppError> {
        if !self.vec1_ready(generation)? {
            return Err(AppError::search_not_ready(
                "Vec1 benchmark projection is stale",
            ));
        }
        let mut result = Vec::new();
        let mut eligible = 0;
        let mut timings = Timings::default();
        for connector in [false, true] {
            let started = std::time::Instant::now();
            let selection = self.vec1_selection(
                connector,
                None,
                None,
                Some(filters),
                &filter::Plan::Direct(vec![Vec::new()]),
            )?;
            timings.eligibility_ms += started.elapsed().as_secs_f64() * 1000.0;
            eligible += selection.count;
            if selection.count == 0 {
                continue;
            }
            let started = std::time::Instant::now();
            let candidates = limit.saturating_mul(4);
            timings.ann_branches += 1;
            let options = format!("{{K:{candidates},streaming:1,nprobe:0.1}}");
            let sql = format!("SELECT p.target_id,p.norm,p.vector FROM views.vec1_index(:query,:options) v
                CROSS JOIN vec1_points p ON p.id=v.rowid {} WHERE v.connector={} AND {} LIMIT :limit",
                selection.legacy_joins, i32::from(connector), selection.legacy_predicate);
            let mut statement = self.connection.prepare(&sql).map_err(AppError::database)?;
            let mut rows = statement.query(rusqlite::named_params!{
                ":query":float_blob(query), ":options":options, ":cutoff":Option::<i64>::None,
                ":provider":Option::<String>::None, ":limit":i64::try_from(candidates).map_err(|_| AppError::internal("benchmark limit overflow"))?,
            }).map_err(AppError::database)?;
            while let Some(row) = rows.next().map_err(AppError::database)? {
                result.push(Candidate {
                    connector,
                    id: row.get(0).map_err(AppError::database)?,
                    norm: row.get(1).map_err(AppError::database)?,
                    vector: row.get(2).map_err(AppError::database)?,
                });
            }
            timings.ann_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        Ok((result, eligible, timings))
    }
}
