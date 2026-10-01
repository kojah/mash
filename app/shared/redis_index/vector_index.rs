//! Typed Redis vector schema and runtime policy. Defaults retain exact retrieval.
use super::fields;
use crate::AppError;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "algorithm", rename_all = "kebab-case", deny_unknown_fields)]
pub(in crate::shared) enum VectorIndex {
    Exact {},
    Hnsw {
        #[serde(default = "default_m")]
        m: usize,
        #[serde(default = "default_construction")]
        ef_construction: usize,
        #[serde(default = "default_runtime")]
        ef_runtime: usize,
        #[serde(default)]
        filter_policy: FilterPolicy,
        batch_size: Option<usize>,
    },
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::Exact {}
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::shared) enum FilterPolicy {
    #[default]
    Auto,
    Exact,
    Batches,
}

const fn default_m() -> usize {
    16
}
const fn default_construction() -> usize {
    200
}
const fn default_runtime() -> usize {
    200
}

impl VectorIndex {
    pub(in crate::shared) fn validate(self) -> Result<(), AppError> {
        if let Self::Hnsw {
            m,
            ef_construction,
            ef_runtime,
            filter_policy,
            batch_size,
        } = self
        {
            if !(2..=128).contains(&m)
                || !(4..=4096).contains(&ef_construction)
                || !(1..=4096).contains(&ef_runtime)
            {
                return Err(AppError::configuration(
                    "Redis HNSW requires m=2..128, ef_construction=4..4096 and ef_runtime=1..4096",
                ));
            }
            if batch_size.is_some_and(|size| !(1..=4096).contains(&size))
                || (batch_size.is_some() && filter_policy != FilterPolicy::Batches)
            {
                return Err(AppError::configuration(
                    "Redis HNSW batch_size requires filter_policy=batches and a value in 1..4096",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn algorithm(self) -> &'static str {
        match self {
            Self::Exact {} => "FLAT",
            Self::Hnsw { .. } => "HNSW",
        }
    }

    pub(super) fn attributes(self, dimensions: usize) -> Vec<String> {
        let mut values = vec![
            "TYPE".into(),
            "FLOAT32".into(),
            "DIM".into(),
            dimensions.to_string(),
            "DISTANCE_METRIC".into(),
            "COSINE".into(),
        ];
        if let Self::Hnsw {
            m, ef_construction, ..
        } = self
        {
            values.extend([
                "M".into(),
                m.to_string(),
                "EF_CONSTRUCTION".into(),
                ef_construction.to_string(),
            ]);
        }
        values
    }

    pub(super) fn query_attributes(self) -> String {
        let Self::Hnsw {
            ef_runtime,
            filter_policy,
            batch_size,
            ..
        } = self
        else {
            return String::new();
        };
        if filter_policy == FilterPolicy::Exact {
            return "=>{$HYBRID_POLICY: ADHOC_BF}".into();
        }
        let mut values = vec![format!("$EF_RUNTIME: {ef_runtime}")];
        if filter_policy == FilterPolicy::Batches {
            values.push("$HYBRID_POLICY: BATCHES".into());
        }
        if let Some(size) = batch_size {
            values.push(format!("$BATCH_SIZE: {size}"));
        }
        format!("=>{{{}}}", values.join("; "))
    }

    pub(super) async fn create(
        self,
        connection: &mut redis::aio::ConnectionManager,
        name: &str,
        prefix: &str,
        dimensions: usize,
    ) -> redis::RedisResult<()> {
        let attributes = self.attributes(dimensions);
        redis::cmd("FT.CREATE")
            .arg(name)
            .arg("ON")
            .arg("HASH")
            .arg("PREFIX")
            .arg(1)
            .arg(prefix)
            .arg("STOPWORDS")
            .arg(0)
            .arg("SCHEMA")
            .arg(fields::VERSION)
            .arg("TAG")
            .arg(fields::CONVERSATION)
            .arg("TAG")
            .arg(fields::PROVIDER_TAG)
            .arg("TAG")
            .arg(fields::ORDINAL)
            .arg("NUMERIC")
            .arg("SORTABLE")
            .arg(fields::CREATED_AT)
            .arg("NUMERIC")
            .arg(fields::SEARCHABLE)
            .arg("NUMERIC")
            .arg(fields::PROJECTION)
            .arg("TEXT")
            .arg("NOSTEM")
            .arg(fields::VECTOR)
            .arg("VECTOR")
            .arg(self.algorithm())
            .arg(attributes.len())
            .arg(attributes)
            .query_async(connection)
            .await
    }

    pub(super) async fn wait_until_ready(
        self,
        connection: &mut redis::aio::ConnectionManager,
        name: &str,
        dimensions: usize,
    ) -> Result<(), AppError> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            let info = redis::cmd("FT.INFO")
                .arg(name)
                .query_async::<redis::Value>(connection)
                .await
                .map_err(super::redis_error)?;
            self.verify(&info, dimensions)?;
            if !indexing(&info)? {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(AppError::shared(
                    "Redis vector index initialization timed out; check Redis indexing status before restarting the service",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    pub(super) fn verify(self, info: &redis::Value, dimensions: usize) -> Result<(), AppError> {
        let invalid = || {
            AppError::configuration(
                "Redis vector schema differs from redis_vector_index or embedding dimensions; restore matching settings or choose a new group_id and republish indexes",
            )
        };
        let attributes = property(info, "attributes").ok_or_else(invalid)?;
        let redis::Value::Array(attributes) = attributes else {
            return Err(invalid());
        };
        let vectors = attributes
            .iter()
            .filter(|item| text(property(item, "identifier")) == Some("vector"))
            .collect::<Vec<_>>();
        if vectors.len() != 1 {
            return Err(invalid());
        }
        let vector = vectors[0];
        if text(property(vector, "type")) != Some("VECTOR")
            || text(property(vector, "algorithm")) != Some(self.algorithm())
            || text(property(vector, "data_type")) != Some("FLOAT32")
            || text(property(vector, "distance_metric")) != Some("COSINE")
            || number(property(vector, "dim")) != Some(dimensions)
        {
            return Err(invalid());
        }
        if let Self::Hnsw {
            m, ef_construction, ..
        } = self
            && (number(property(vector, "M")) != Some(m)
                || number(property(vector, "ef_construction")) != Some(ef_construction))
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn property<'a>(value: &'a redis::Value, name: &str) -> Option<&'a redis::Value> {
    let redis::Value::Array(values) = value else {
        return None;
    };
    let (pairs, remainder) = values.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let mut matching = pairs
        .iter()
        .filter(|pair| text(Some(&pair[0])) == Some(name));
    let found = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    Some(&found[1])
}

pub(super) fn indexing(info: &redis::Value) -> Result<bool, AppError> {
    match number(property(info, "indexing")) {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(AppError::shared(
            "invalid Redis index initialization status",
        )),
    }
}

fn text(value: Option<&redis::Value>) -> Option<&str> {
    match value? {
        redis::Value::BulkString(bytes) => std::str::from_utf8(bytes).ok(),
        redis::Value::SimpleString(value) => Some(value),
        _ => None,
    }
}

fn number(value: Option<&redis::Value>) -> Option<usize> {
    match value? {
        redis::Value::Int(value) => usize::try_from(*value).ok(),
        value => text(Some(value))?.parse().ok(),
    }
}
