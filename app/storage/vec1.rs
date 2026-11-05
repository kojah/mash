//! Experimental local IVF projection. Every shadow write and its certificate
//! use the existing views transaction; readers never repair or train indexes.
use rusqlite::{Connection, params};

use super::Storage;
use crate::AppError;

mod filter;

const INDEX: &str = "CREATE VIRTUAL TABLE views.vec1_index USING vec1(vector, connector, point_key, created_at, provider, role, chat, project, repository, source, kind);";

struct Selection {
    branches: Vec<Vec<String>>,
    count: usize,
    keys: Option<String>,
    #[cfg(test)]
    legacy_joins: String,
    #[cfg(test)]
    legacy_predicate: String,
}

#[derive(Default, serde::Serialize)]
pub(crate) struct Timings {
    pub(crate) eligibility_ms: f64,
    pub(crate) ann_ms: f64,
    pub(crate) ann_branches: usize,
    pub(crate) eligible_key_queries: usize,
}

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
         CREATE VIRTUAL TABLE {schema}.vec1_index USING vec1(vector, connector, point_key, created_at, provider, role, chat, project, repository, source, kind);")
    ).map_err(AppError::database)
}

pub(super) fn track_changes(connection: &Connection) -> Result<(), AppError> {
    for (schema, table) in [
        ("views", "message_embeddings"),
        ("views", "connector_embeddings"),
        ("views", "connector_units"),
        ("main", "objects"),
    ] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            connection
                .execute_batch(&format!(
                    "CREATE TEMP TRIGGER IF NOT EXISTS vec1_dirty_{table}_{operation}
                 AFTER {operation} ON {schema}.{table} BEGIN
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
            .execute_batch(&format!(
                "DROP TABLE views.vec1_index; {INDEX} DELETE FROM vec1_points;"
            ))
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
        self.populate_vec1_metadata()?;
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

    fn populate_vec1_metadata(&self) -> Result<(), AppError> {
        let mut rows = self
            .connection
            .prepare("SELECT p.id,p.connector,p.norm,p.vector,m.created_at,c.source,m.role,m.conversation_id,c.project,c.repository,NULL,m.kind
                FROM vec1_points p JOIN objects m ON m.storage_id=p.target_id
                JOIN objects c ON c.id=m.conversation_id AND c.origin='provider' AND c.object_role='container'
                WHERE p.connector=0
                UNION ALL SELECT p.id,p.connector,p.norm,p.vector,u.created_at,NULL,NULL,u.group_anchor,NULL,NULL,u.source_id,u.label
                FROM vec1_points p JOIN connector_units u ON u.id=p.target_id WHERE p.connector=1")
            .map_err(AppError::database)?;
        let mut insert = self
            .connection
            .prepare("INSERT INTO vec1_index(rowid,vector,connector,point_key,created_at,provider,role,chat,project,repository,source,kind) VALUES(?1,?2,?3,?1,?4,?5,?6,?7,?8,?9,?10,?11)")
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
                    row.get::<_, bool>(1).map_err(AppError::database)?,
                    row.get::<_, Option<i64>>(4).map_err(AppError::database)?,
                    row.get::<_, Option<String>>(5)
                        .map_err(AppError::database)?,
                    row.get::<_, Option<String>>(6)
                        .map_err(AppError::database)?,
                    row.get::<_, Option<String>>(7)
                        .map_err(AppError::database)?,
                    row.get::<_, Option<String>>(8)
                        .map_err(AppError::database)?,
                    row.get::<_, Option<String>>(9)
                        .map_err(AppError::database)?,
                    row.get::<_, Option<String>>(10)
                        .map_err(AppError::database)?,
                    row.get::<_, Option<String>>(11)
                        .map_err(AppError::database)?
                ])
                .map_err(AppError::database)?;
        }
        drop(cursor);
        drop(rows);
        drop(insert);
        Ok(())
    }

    fn vec1_selection(
        &self,
        connector: bool,
        cutoff: Option<i64>,
        provider: Option<crate::provider::Provider>,
        filters: Option<&crate::search_filter::Scope>,
        plan: &filter::Plan,
    ) -> Result<Selection, AppError> {
        let native_filter = filters.map_or_else(|| "TRUE".into(), |f| f.filter.sql(Some(&f.node)));
        let connector_filter =
            filters.map_or_else(|| "TRUE".into(), |f| f.filter.connector_sql(Some(&f.node)));
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
        let mut eligible_json = None;
        let native_terms = match &plan {
            filter::Plan::Direct(branches) => {
                let mut terms = Vec::new();
                if let Some(cutoff) = cutoff {
                    terms.push(format!("v.created_at >= {cutoff}"));
                }
                if let Some(provider) = provider {
                    terms.push(format!("v.provider = {}", filter::quote(provider.as_str())));
                }
                branches
                    .iter()
                    .map(|branch| {
                        let mut branch = branch.clone();
                        branch.extend(terms.iter().cloned());
                        branch
                    })
                    .collect()
            }
            filter::Plan::EligibleKeys => {
                let mut statement = self.connection.prepare(&format!(
                    "SELECT p.id FROM vec1_points p {joins} WHERE p.connector={} AND {predicate}", i32::from(connector)
                )).map_err(AppError::database)?;
                let keys = statement.query_map(rusqlite::named_params!{":cutoff":cutoff,":provider":provider.map(crate::provider::Provider::as_str)}, |row| row.get::<_, i64>(0))
                    .map_err(AppError::database)?.collect::<Result<Vec<_>, _>>().map_err(AppError::database)?;
                eligible_json = Some((
                    keys.len(),
                    serde_json::to_string(&keys)
                        .map_err(|_| AppError::internal("serialize Vec1 eligible keys"))?,
                ));
                vec![vec![
                    "v.point_key IN (SELECT value FROM json_each(:eligible))".into(),
                ]]
            }
            filter::Plan::Empty => unreachable!(),
        };
        let count: i64 = if let Some((count, _)) = &eligible_json {
            i64::try_from(*count).map_err(|_| AppError::internal("too many Vec1 eligible keys"))?
        } else {
            self.connection.query_row(&format!(
            "SELECT count(*) FROM vec1_points p {joins} WHERE p.connector={} AND {predicate}",
            i32::from(connector)), rusqlite::named_params!{":cutoff":cutoff,":provider":provider.map(crate::provider::Provider::as_str)}, |row| row.get(0))
            .map_err(AppError::database)?
        };
        Ok(Selection {
            count: usize::try_from(count)
                .map_err(|_| AppError::database_data("negative Vec1 eligible count"))?,
            branches: native_terms,
            keys: eligible_json.map(|(_, json)| json),
            #[cfg(test)]
            legacy_joins: joins,
            #[cfg(test)]
            legacy_predicate: predicate,
        })
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
        let (candidates, eligible, _) =
            self.vec1_candidates_profiled(generation, query, provider, days, filters, limit)?;
        Ok((candidates, eligible))
    }

    pub(crate) fn vec1_candidates_profiled(
        &self,
        generation: &str,
        query: &[u8],
        provider: Option<crate::provider::Provider>,
        days: Option<u32>,
        filters: Option<&crate::search_filter::Scope>,
        limit: usize,
    ) -> Result<(Vec<Candidate>, usize, Timings), AppError> {
        self.vec1_profiled(generation, query, (provider, days, filters), limit, false)
    }

    fn vec1_profiled(
        &self,
        generation: &str,
        query: &[u8],
        (provider, days, filters): (
            Option<crate::provider::Provider>,
            Option<u32>,
            Option<&crate::search_filter::Scope>,
        ),
        limit: usize,
        streaming: bool,
    ) -> Result<(Vec<Candidate>, usize, Timings), AppError> {
        if !self.vec1_ready(generation)? {
            return Err(AppError::search_not_ready(
                "Vec1 views need an explicit index rebuild",
            ));
        }
        let cutoff = super::cutoff_timestamp(days)?;

        let plan = filters.map_or_else(
            || filter::Plan::Direct(vec![Vec::new()]),
            |scope| filter::plan(&scope.filter, &scope.node),
        );
        if matches!(plan, filter::Plan::Empty) {
            return Ok((Vec::new(), 0, Timings::default()));
        }
        let mut result = Vec::new();
        let mut eligible = 0_usize;
        let mut timings = Timings::default();
        for connector in [false, true] {
            if connector && provider.is_some() {
                continue;
            }
            let started = std::time::Instant::now();
            let selection = self.vec1_selection(connector, cutoff, provider, filters, &plan)?;
            timings.eligibility_ms += started.elapsed().as_secs_f64() * 1000.0;
            eligible += selection.count;
            if selection.count == 0 {
                continue;
            }
            let candidates = limit.saturating_mul(4).max(limit);
            let started = std::time::Instant::now();
            timings.ann_branches += selection.branches.len();
            timings.eligible_key_queries += usize::from(selection.keys.is_some());
            let options = format!(
                "{{K:{candidates},streaming:{},nprobe:0.1}}",
                i32::from(streaming)
            );
            let sql = candidate_sql(connector, &selection.branches);
            let mut statement = self.connection.prepare(&sql).map_err(AppError::database)?;
            let vector = float_blob(query);
            let limit = i64::try_from(candidates)
                .map_err(|_| AppError::internal("candidate limit too large"))?;
            let eligible = selection.keys;
            let bindings: [(&str, &dyn rusqlite::ToSql); 4] = [
                (":query", &vector),
                (":options", &options),
                (":limit", &limit),
                (":eligible", &eligible),
            ];
            for (name, value) in bindings {
                if let Some(index) = statement
                    .parameter_index(name)
                    .map_err(AppError::database)?
                {
                    statement
                        .raw_bind_parameter(index, value)
                        .map_err(AppError::database)?;
                }
            }
            let mut cursor = statement.raw_query();
            while let Some(row) = cursor.next().map_err(AppError::database)? {
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

fn metadata_predicate(connector: bool, terms: &[String]) -> String {
    let predicate = if terms.is_empty() {
        String::new()
    } else {
        format!(" AND {}", terms.join(" AND "))
    };
    format!("v.connector={}{predicate}", i32::from(connector))
}

fn candidate_sql(connector: bool, branches: &[Vec<String>]) -> String {
    if branches.len() == 1 {
        return format!(
            "SELECT p.target_id,p.norm,p.vector FROM views.vec1_index(:query,:options) v
            CROSS JOIN vec1_points p ON p.id=v.rowid WHERE {} LIMIT :limit",
            metadata_predicate(connector, &branches[0])
        );
    }
    // Each branch filters inside ANN. Merge and deduplicate before applying the
    // same global candidate budget as a single search; overlapping OR branches
    // must not crowd distinct results out of the rescoring pool.
    let ctes = branches.iter().enumerate().map(|(index, terms)| format!(
        "b{index} AS MATERIALIZED (SELECT v.rowid AS point_id,v.distance AS d FROM views.vec1_index(:query,:options) v WHERE {} LIMIT :limit)", metadata_predicate(connector,terms)
    )).collect::<Vec<_>>().join(",");
    let union = (0..branches.len())
        .map(|index| format!("SELECT * FROM b{index}"))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    format!("WITH {ctes} SELECT p.target_id,p.norm,p.vector FROM
        (SELECT point_id,min(d) AS nearest_distance FROM ({union}) GROUP BY point_id ORDER BY nearest_distance,point_id LIMIT :limit) r
        CROSS JOIN vec1_points p ON p.id=r.point_id")
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

#[cfg(test)]
mod benchmarks;
