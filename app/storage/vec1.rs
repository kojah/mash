//! Experimental local IVF projection. Every shadow write and its certificate
//! use the existing views transaction; readers never repair or train indexes.
use rusqlite::{Connection, params};

use super::Storage;
use crate::AppError;

pub(crate) struct Candidate {
    pub(crate) connector: bool,
    pub(crate) id: i64,
    pub(crate) norm: f32,
    pub(crate) vector: Vec<u8>,
}

pub(super) fn create(connection: &Connection, schema: &str) -> Result<(), AppError> {
    connection.execute_batch(&format!(
        "CREATE TABLE {schema}.vec1_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1), generation TEXT, dirty INTEGER NOT NULL);
         INSERT INTO {schema}.vec1_state VALUES(1,NULL,1);
         CREATE TABLE {schema}.vec1_points(id INTEGER PRIMARY KEY, connector INTEGER NOT NULL, target_id INTEGER NOT NULL, norm REAL NOT NULL, vector BLOB NOT NULL);
         CREATE UNIQUE INDEX {schema}.vec1_points_target ON vec1_points(connector,target_id);
         CREATE VIRTUAL TABLE {schema}.vec1_index USING vec1(vector, connector);")
    ).map_err(AppError::database)
}

pub(super) fn track_changes(connection: &Connection) -> Result<(), AppError> {
    for table in ["message_embeddings", "connector_embeddings"] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            connection
                .execute_batch(&format!(
                    "CREATE TEMP TRIGGER IF NOT EXISTS vec1_dirty_{table}_{operation}
                 AFTER {operation} ON views.{table} BEGIN
                 UPDATE vec1_state SET dirty=1 WHERE singleton=1; END;"
                ))
                .map_err(AppError::database)?;
        }
    }
    Ok(())
}

impl Storage {
    pub(crate) fn vec1_ready(&self, generation: &str) -> Result<bool, AppError> {
        self.connection
            .query_row(
                "SELECT dirty=0 AND generation IS ?1 FROM vec1_state WHERE singleton=1",
                [generation],
                |row| row.get(0),
            )
            .map_err(AppError::database)
    }

    /// This first trial rebuilds on changed vectors, then trains once. Keep the
    /// exact artifacts for controlled comparison; incremental IVF maintenance
    /// and dropping duplicate packed vectors require separate measurements.
    #[tracing::instrument(name = "vec1_build", skip_all)]
    pub(crate) fn sync_vec1(&self, generation: &str) -> Result<(), AppError> {
        self.require_writer()?;
        if self.vec1_ready(generation)? {
            return Ok(());
        }
        self.connection
            .execute_batch(
                "DROP TABLE views.vec1_index;
             CREATE VIRTUAL TABLE views.vec1_index USING vec1(vector, connector);
             DELETE FROM vec1_points;",
            )
            .map_err(AppError::database)?;
        self.connection
            .execute(
                "INSERT INTO vec1_points(connector,target_id,norm,vector)
             SELECT 0,m.storage_id,e.norm,e.vector FROM message_embeddings e
             JOIN objects m ON m.id=e.message_id AND m.origin='provider' AND m.object_role='entry'
             WHERE e.generation=?1
             UNION ALL SELECT 1,u.id,e.norm,e.vector FROM connector_embeddings e
             JOIN connector_units u ON u.id=e.unit_id WHERE e.generation=?1",
                [generation],
            )
            .map_err(AppError::database)?;
        let mut rows = self
            .connection
            .prepare("SELECT id,connector,norm,vector FROM vec1_points ORDER BY id")
            .map_err(AppError::database)?;
        let mut insert = self
            .connection
            .prepare("INSERT INTO vec1_index(rowid,vector,connector) VALUES(?1,?2,?3)")
            .map_err(AppError::database)?;
        let mut cursor = rows.query([]).map_err(AppError::database)?;
        while let Some(row) = cursor.next().map_err(AppError::database)? {
            let vector: Vec<u8> = row.get(3).map_err(AppError::database)?;
            let norm: f64 = row.get(2).map_err(AppError::database)?;
            if vector.is_empty() || !norm.is_finite() || norm < 0.0 {
                return Err(AppError::database_data("invalid Vec1 source vector"));
            }
            insert
                .execute(params![
                    row.get::<_, i64>(0).map_err(AppError::database)?,
                    float_blob(&vector),
                    row.get::<_, bool>(1).map_err(AppError::database)?
                ])
                .map_err(AppError::database)?;
        }
        drop(cursor);
        drop(rows);
        drop(insert);
        let count: i64 = self
            .connection
            .query_row("SELECT count(*) FROM vec1_points", [], |row| row.get(0))
            .map_err(AppError::database)?;
        if count >= 512 {
            // Fixed first-trial policy: IVF only, no lossy PQ. Bound training
            // memory with a deterministic sample spread over both source types.
            let buckets = count.isqrt().clamp(2, 512);
            let options = format!("{{distance:'cos',nbucket:{buckets},quantizer:'none'}}");
            let model: Vec<u8> = self
                .connection
                .query_row(
                    "SELECT vec1_train(vector,?1) FROM
                 (SELECT vector FROM vec1_index WHERE rowid % ?2 = 0 ORDER BY rowid LIMIT 50000)",
                    params![options, 1 + (count - 1) / 50_000],
                    |row| row.get(0),
                )
                .map_err(AppError::database)?;
            self.connection
                .execute(
                    "INSERT INTO vec1_index(cmd,arg) VALUES('rebuild',?1)",
                    [model],
                )
                .map_err(AppError::database)?;
        } else {
            if count == 0 {
                // Vec1 0.8 cannot drop a recreated, never-initialized table
                // again in the same transaction. Establish its vector shape,
                // then remove the seed before certifying the empty projection.
                self.connection
                    .execute(
                        "INSERT INTO vec1_index(rowid,vector,connector) VALUES(0,?1,0)",
                        [float_blob(&[1, 0])],
                    )
                    .map_err(AppError::database)?;
                self.connection
                    .execute("DELETE FROM vec1_index WHERE rowid=0", [])
                    .map_err(AppError::database)?;
            }
            self.connection.execute_batch(
                "INSERT INTO vec1_index(cmd,arg) VALUES('rebuild','{index:\"flat\",distance:\"cos\"}')"
            ).map_err(AppError::database)?;
        }
        self.connection
            .execute(
                "UPDATE vec1_state SET generation=?1,dirty=0 WHERE singleton=1",
                [generation],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn vec1_candidates(
        &self,
        generation: &str,
        query: &[u8],
        provider: Option<crate::provider::Provider>,
        days: Option<u32>,
        filters: Option<&crate::search_filter::Scope>,
        limit: usize,
    ) -> Result<(Vec<Candidate>, usize), AppError> {
        if !self.vec1_ready(generation)? {
            return Err(AppError::search_not_ready(
                "Vec1 views need an explicit index rebuild",
            ));
        }
        let cutoff = super::cutoff_timestamp(days)?;
        let native_filter = filters.map_or_else(|| "TRUE".into(), |f| f.filter.sql(Some(&f.node)));
        let connector_filter =
            filters.map_or_else(|| "TRUE".into(), |f| f.filter.connector_sql(Some(&f.node)));
        let mut result = Vec::new();
        let mut eligible = 0_usize;
        for connector in [false, true] {
            if connector && provider.is_some() {
                continue;
            }
            let (joins, predicate) = if connector {
                (
                    "CROSS JOIN connector_units u ON u.id=p.target_id".to_owned(),
                    format!(
                        "(:cutoff IS NULL OR u.created_at>=:cutoff) AND (:provider IS NULL) AND ({connector_filter})"
                    ),
                )
            } else {
                ("CROSS JOIN objects m ON m.storage_id=p.target_id
                  CROSS JOIN objects c ON c.origin='provider' AND c.object_role='container' AND c.id=m.conversation_id".to_owned(),
                 format!("(:cutoff IS NULL OR m.created_at>=:cutoff) AND (:provider IS NULL OR c.source=:provider) AND ({native_filter})"))
            };
            let count: i64 = self.connection.query_row(&format!(
                "SELECT count(*) FROM vec1_points p {joins} WHERE p.connector={} AND {predicate}",
                i32::from(connector)), rusqlite::named_params!{":cutoff":cutoff,":provider":provider.map(crate::provider::Provider::as_str)}, |row| row.get(0))
                .map_err(AppError::database)?;
            eligible += usize::try_from(count)
                .map_err(|_| AppError::database_data("negative Vec1 eligible count"))?;
            if count == 0 {
                continue;
            }
            let candidates = limit.saturating_mul(4).max(limit);
            let options = format!("{{K:{candidates},streaming:1,nprobe:0.1}}");
            let sql = format!(
                "SELECT p.target_id,p.norm,p.vector FROM views.vec1_index(:query,:options) v
                 CROSS JOIN vec1_points p ON p.id=v.rowid {joins}
                 WHERE v.connector={} AND {predicate} LIMIT :limit",
                i32::from(connector)
            );
            let mut statement = self.connection.prepare(&sql).map_err(AppError::database)?;
            let mut cursor = statement.query(rusqlite::named_params! {
                ":query":float_blob(query), ":options":options, ":cutoff":cutoff,
                ":provider":provider.map(crate::provider::Provider::as_str),
                ":limit":i64::try_from(candidates).map_err(|_|AppError::internal("candidate limit too large"))?,
            }).map_err(AppError::database)?;
            while let Some(row) = cursor.next().map_err(AppError::database)? {
                result.push(Candidate {
                    connector,
                    id: row.get(0).map_err(AppError::database)?,
                    norm: row.get(1).map_err(AppError::database)?,
                    vector: row.get(2).map_err(AppError::database)?,
                });
            }
        }
        Ok((result, eligible))
    }
}

fn float_blob(vector: &[u8]) -> Vec<u8> {
    // Vec1 requires at least two dimensions. Zero-padding one-dimensional
    // embeddings preserves their norm and cosine without narrowing Mash's API.
    vector
        .iter()
        .copied()
        .chain(std::iter::repeat_n(0, 2_usize.saturating_sub(vector.len())))
        .flat_map(|value| f32::from(i8::from_ne_bytes([value])).to_ne_bytes())
        .collect()
}

#[cfg(test)]
mod tests;
