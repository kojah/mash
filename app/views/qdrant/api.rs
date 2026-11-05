//! Typed REST reads/setup and protobuf publication. Upstream bodies and secrets
//! never enter errors; gold mutations have no automatic retry or REST fallback.
use super::{AppError, MemoryPolicy, MemoryTier, PAYLOAD_MEMORY, PhysicalMemory, failure};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::Instrument;
mod grpc;
mod optimization;
mod planner;
#[cfg(test)]
mod schema_tests;
mod staging;
pub(super) use staging::StagingBatch;

pub(crate) fn endpoint(url: &str) -> Result<reqwest::Url, AppError> {
    let mut url = reqwest::Url::parse(url)
        .map_err(|_| AppError::configuration("invalid Qdrant HTTP endpoint"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(AppError::configuration(
            "Qdrant endpoint requires http(s) origin without credentials, path, query, or fragment",
        ));
    }
    url.set_path("/");
    Ok(url)
}

#[derive(Clone)]
pub(super) struct Client {
    mutations: Option<grpc::Client>,
    http: reqwest::Client,
    url: reqwest::Url,
    max_response: usize,
}
#[derive(Deserialize)]
struct Envelope<T> {
    result: T,
    status: ResponseStatus,
    time: Option<f64>,
    usage: Option<Usage>,
}
#[derive(Deserialize)]
struct Usage {
    hardware: Option<HardwareUsage>,
}
#[derive(Deserialize)]
struct HardwareUsage {
    cpu: Option<u64>,
    payload_io_read: Option<u64>,
    payload_io_write: Option<u64>,
    payload_index_io_read: Option<u64>,
    payload_index_io_write: Option<u64>,
    vector_io_read: Option<u64>,
    vector_io_write: Option<u64>,
}
impl<T> Envelope<T> {
    fn record_metrics(&self) {
        let span = tracing::Span::current();
        if let Some(time) = self.time.filter(|time| time.is_finite() && *time >= 0.0) {
            span.record("qdrant_time_seconds", time);
        }
        if let Some(hardware) = self
            .usage
            .as_ref()
            .and_then(|usage| usage.hardware.as_ref())
        {
            // Native accounting units, not CPU seconds or physical disk bytes.
            for (field, value) in [
                ("qdrant_cpu", hardware.cpu),
                ("qdrant_payload_io_read", hardware.payload_io_read),
                ("qdrant_payload_io_write", hardware.payload_io_write),
                (
                    "qdrant_payload_index_io_read",
                    hardware.payload_index_io_read,
                ),
                (
                    "qdrant_payload_index_io_write",
                    hardware.payload_index_io_write,
                ),
                ("qdrant_vector_io_read", hardware.vector_io_read),
                ("qdrant_vector_io_write", hardware.vector_io_write),
            ] {
                if let Some(value) = value {
                    span.record(field, value);
                }
            }
        }
    }
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ResponseStatus {
    Ok,
}
#[derive(Serialize)]
pub(super) struct Point<P> {
    pub id: String,
    pub vector: Vectors,
    pub payload: P,
}
#[derive(Deserialize)]
pub(super) struct Record<P> {
    pub id: String,
    pub payload: P,
}
#[derive(Deserialize)]
pub(super) struct Scored<P> {
    pub id: String,
    pub score: f32,
    pub payload: P,
}
#[derive(Clone, Default)]
pub(super) struct Vectors {
    pub dense: Option<(String, Vec<f64>)>,
    pub lexical: Option<Sparse>,
}
impl Serialize for Vectors {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        if let Some((name, vector)) = &self.dense {
            map.serialize_entry(name, vector)?;
        }
        if let Some(vector) = &self.lexical {
            map.serialize_entry("lexical", vector)?;
        }
        map.end()
    }
}
#[derive(Clone, Serialize)]
pub(super) struct Sparse {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
}
#[derive(Serialize)]
pub(super) struct Upsert<P> {
    pub points: Vec<Point<P>>,
}
#[derive(Clone, Default, Serialize)]
pub(super) struct Filter {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub must: Vec<Condition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub should: Vec<Condition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub must_not: Vec<Condition>,
}
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub(super) enum Condition {
    Match { key: String, r#match: Match },
    Range { key: String, range: Range },
    Nested(Filter),
}
#[derive(Clone, Serialize)]
pub(super) struct Match {
    pub any: Vec<String>,
}
#[derive(Clone, Default, Serialize)]
pub(super) struct Range {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gte: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lt: Option<i64>,
}
pub(super) fn matches(key: &str, values: Vec<String>) -> Condition {
    Condition::Match {
        key: key.into(),
        r#match: Match { any: values },
    }
}
// An impossible literal in a private field represents an empty OR/list.
pub(super) fn impossible() -> Condition {
    matches("version", vec!["!".into()])
}
#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum QueryVector {
    Dense(Vec<f64>),
    Sparse(Sparse),
}
#[derive(Serialize)]
pub(super) struct QueryRequest {
    pub query: QueryVector,
    pub using: String,
    pub filter: Filter,
    pub params: Params,
    pub limit: usize,
    pub with_payload: bool,
}
#[derive(Serialize)]
pub(super) struct Params {
    pub exact: bool,
    pub hnsw_ef: usize,
}
#[derive(Deserialize)]
struct QueryResult<P> {
    points: Vec<Scored<P>>,
}
#[derive(Serialize)]
struct Retrieve<'a> {
    ids: &'a [String],
    with_payload: bool,
    with_vector: bool,
}
#[derive(Serialize)]
struct Scroll {
    filter: Filter,
    limit: usize,
    with_payload: Payload,
    with_vector: bool,
    offset: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(untagged)]
enum Payload {
    Fields(&'static [&'static str]),
}
#[derive(Deserialize)]
pub(super) struct ScrollResult<P> {
    pub points: Vec<Record<P>>,
    pub next_page_offset: Option<String>,
}
#[derive(Deserialize)]
struct Operation {
    #[serde(rename = "status")]
    _status: OperationStatus,
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum OperationStatus {
    Completed,
    Acknowledged,
}
#[derive(Serialize, Deserialize)]
struct DenseConfig {
    size: usize,
    distance: String,
    #[serde(flatten)]
    placement: Placement,
    #[serde(default, skip_serializing)]
    quantization_config: Option<serde::de::IgnoredAny>,
    #[serde(default, skip_serializing)]
    hnsw_config: Option<serde::de::IgnoredAny>,
    #[serde(default, skip_serializing)]
    multivector_config: Option<serde::de::IgnoredAny>,
    #[serde(default, skip_serializing)]
    datatype: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct SparseConfig {
    modifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index: Option<Placement>,
}
#[derive(Serialize, Deserialize)]
struct Hnsw {
    m: usize,
    ef_construct: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    full_scan_threshold: Option<usize>,
    #[serde(flatten)]
    placement: Placement,
}

#[derive(Serialize, Deserialize)]
struct Placement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory: Option<PhysicalMemory>,
    // Qdrant may still report this deprecated field; explicit memory takes precedence.
    #[serde(default, skip_serializing)]
    on_disk: Option<bool>,
}

impl Placement {
    fn new(memory: PhysicalMemory) -> Self {
        Self {
            memory: Some(memory),
            on_disk: None,
        }
    }

    fn matches(&self, expected: PhysicalMemory) -> bool {
        self.memory.unwrap_or(if self.on_disk == Some(true) {
            PhysicalMemory::Cold
        } else {
            PhysicalMemory::Cached
        }) == expected
    }

    fn matches_sparse(&self, expected: PhysicalMemory) -> bool {
        self.memory.unwrap_or(if self.on_disk == Some(true) {
            PhysicalMemory::Cold
        } else {
            PhysicalMemory::Pinned
        }) == expected
    }
}
#[derive(Serialize)]
struct Create {
    vectors: BTreeMap<String, DenseConfig>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    sparse_vectors: BTreeMap<String, SparseConfig>,
    hnsw_config: Hnsw,
    #[serde(skip_serializing_if = "Option::is_none")]
    optimizers_config: Option<planner::Optimizers>,
    payload: Placement,
}
#[derive(Deserialize)]
struct Info {
    config: CollectionSettings,
    payload_schema: BTreeMap<String, FieldSchema>,
}
#[derive(Deserialize)]
struct FieldSchema {
    data_type: String,
}
#[derive(Deserialize)]
struct Instance {
    version: String,
}
#[derive(Deserialize)]
struct CollectionSettings {
    params: Parameters,
    hnsw_config: Hnsw,
    #[serde(default, rename = "optimizer_config")]
    optimizers_config: planner::Optimizers,
    #[serde(default)]
    quantization_config: Option<serde::de::IgnoredAny>,
}
#[derive(Deserialize)]
struct Parameters {
    vectors: BTreeMap<String, DenseConfig>,
    #[serde(default)]
    sparse_vectors: Option<BTreeMap<String, SparseConfig>>,
    #[serde(default)]
    payload: Option<Placement>,
    #[serde(default)]
    on_disk_payload: Option<bool>,
}
impl Parameters {
    fn payload_memory(&self) -> PhysicalMemory {
        self.payload.as_ref().and_then(|p| p.memory).unwrap_or(
            if self.on_disk_payload == Some(false) {
                PhysicalMemory::Cached
            } else {
                PhysicalMemory::Cold
            },
        )
    }
}
#[derive(Serialize)]
struct PayloadIndex<'a> {
    field_name: &'a str,
    field_schema: &'a str,
}

fn schema_fields(
    info: &Info,
    dimensions: Option<usize>,
    memory: MemoryTier,
) -> Result<&'static [(&'static str, &'static str)], AppError> {
    schema_fields_mode(info, dimensions, memory, true)
}

fn memory_matches(info: &Info, dimensions: Option<usize>, memory: MemoryTier) -> bool {
    info.config.params.payload_memory() == PAYLOAD_MEMORY
        && (dimensions.is_none()
            || (info
                .config
                .params
                .vectors
                .values()
                .all(|v| v.placement.matches(memory.dense()))
                && info.config.hnsw_config.placement.matches(memory.index())
                && info.config.params.sparse_vectors.as_ref().is_some_and(|s| {
                    s.get("lexical").is_some_and(|v| {
                        v.index
                            .as_ref()
                            .map_or(memory.index() == PhysicalMemory::Pinned, |p| {
                                p.matches_sparse(memory.index())
                            })
                    })
                })))
}

fn schema_fields_mode(
    info: &Info,
    dimensions: Option<usize>,
    memory: MemoryTier,
    check_memory: bool,
) -> Result<&'static [(&'static str, &'static str)], AppError> {
    let vectors = &info.config.params.vectors;
    let valid = if let Some(dimensions) = dimensions {
        dense_name(info, dimensions).is_some()
            && vectors.contains_key("dense")
            && vectors.iter().all(|(name, v)| {
                (name == "dense" || name == &format!("dense_{}", v.size))
                    && (1..=2048).contains(&v.size)
                    && v.placement.memory != Some(PhysicalMemory::Pinned)
                    && v.distance == "Cosine"
                    && v.quantization_config.is_none()
                    && v.hnsw_config.is_none()
                    && v.multivector_config.is_none()
                    && v.datatype.as_deref().is_none_or(|t| t == "float32")
            })
            && info.config.params.sparse_vectors.as_ref().is_some_and(|s| {
                s.len() == 1 && s.get("lexical").is_some_and(|v| v.modifier == "idf")
            })
            && info.config.hnsw_config.m == 16
            && info.config.hnsw_config.ef_construct == 200
            && info.config.quantization_config.is_none()
    } else {
        vectors.is_empty()
            && info
                .config
                .params
                .sparse_vectors
                .as_ref()
                .is_none_or(BTreeMap::is_empty)
    };
    if !valid || info.config.params.payload_memory() == PhysicalMemory::Pinned {
        return Err(AppError::configuration(
            "existing Qdrant collection has incompatible schema; choose a new group_id",
        ));
    }
    let fields: &[(&str, &str)] = if dimensions.is_some() {
        &[
            ("version", "keyword"),
            ("node_id", "keyword"),
            ("provider", "keyword"),
            ("role", "keyword"),
            ("chat_id", "keyword"),
            ("unit.anchor", "keyword"),
            ("project", "keyword"),
            ("repository", "keyword"),
            ("tokens", "keyword"),
            ("created_at", "integer"),
            ("unit.sequence", "integer"),
        ]
    } else {
        &[("kind", "keyword"), ("version", "keyword")]
    };
    for &(name, kind) in fields {
        if info
            .payload_schema
            .get(name)
            .is_some_and(|f| f.data_type != kind)
        {
            return Err(AppError::configuration(
                "existing Qdrant payload index has incompatible schema; choose a new group_id",
            ));
        }
    }
    if check_memory && !memory_matches(info, dimensions, memory) {
        return Err(AppError::configuration(
            "existing Qdrant collection has a different memory policy; use mash serve --activate-memory to change placement without reindexing",
        ));
    }
    Ok(fields)
}

fn dense_name(info: &Info, dimensions: usize) -> Option<String> {
    let vectors = &info.config.params.vectors;
    if vectors.get("dense").is_some_and(|v| v.size == dimensions) {
        Some("dense".into())
    } else {
        let name = format!("dense_{dimensions}");
        vectors
            .get(&name)
            .filter(|v| v.size == dimensions)
            .map(|_| name)
    }
}

impl Client {
    pub(super) async fn verify_version(&self) -> Result<(), AppError> {
        let response = self
            .http
            .get(self.url.clone())
            .send()
            .await
            .map_err(|_| failure())?;
        if !response.status().is_success() {
            return Err(failure());
        }
        let instance: Instance = response.json().await.map_err(|_| failure())?;
        let mut parts = instance.version.split('.');
        let major = parts
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .ok_or_else(failure)?;
        let minor = parts
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .ok_or_else(failure)?;
        if major != 1 || minor < 19 {
            return Err(AppError::configuration(
                "Qdrant backend requires Qdrant 1.19 or later in the 1.x series for conditional publication",
            ));
        }
        Ok(())
    }
    pub(super) fn new(
        url: &str,
        key: Option<&str>,
        max_node_bytes: usize,
    ) -> Result<Self, AppError> {
        let url = endpoint(url)?;
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(key) = key {
            if key.is_empty() {
                return Err(AppError::configuration("Qdrant API key must be nonempty"));
            }
            let mut value = reqwest::header::HeaderValue::from_str(key)
                .map_err(|_| AppError::configuration("invalid Qdrant API key"))?;
            value.set_sensitive(true);
            headers.insert("api-key", value);
        }
        let http = reqwest::Client::builder()
            // Internal database reads usually stay on the same host. Keep
            // their large snapshots free of compression CPU overhead.
            .no_gzip()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| AppError::configuration("could not configure Qdrant HTTP client"))?;
        Ok(Self {
            mutations: None,
            http,
            url,
            max_response: max_node_bytes.saturating_add(16 * 1024 * 1024),
        })
    }
    pub(super) fn with_mutations(mut self, url: &str, key: Option<&str>) -> Result<Self, AppError> {
        self.mutations = Some(grpc::Client::new(url, key)?);
        Ok(self)
    }

    fn mutations(&self) -> Result<&grpc::Client, AppError> {
        self.mutations
            .as_ref()
            .ok_or_else(|| AppError::configuration("Qdrant writer requires a gRPC endpoint"))
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T, AppError> {
        self.call_with_limit(method, path, body, self.max_response)
            .await
    }

    #[tracing::instrument(name = "qdrant_native_http", skip_all, fields(
        method = %method,
        status = tracing::field::Empty,
        response_bytes = tracing::field::Empty,
        qdrant_time_seconds = tracing::field::Empty,
        qdrant_cpu = tracing::field::Empty,
        qdrant_payload_io_read = tracing::field::Empty,
        qdrant_payload_io_write = tracing::field::Empty,
        qdrant_payload_index_io_read = tracing::field::Empty,
        qdrant_payload_index_io_write = tracing::field::Empty,
        qdrant_vector_io_read = tracing::field::Empty,
        qdrant_vector_io_write = tracing::field::Empty
    ))]
    async fn call_with_limit<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
        limit: usize,
    ) -> Result<T, AppError> {
        // Trace only phases and scalar counts. URLs, collection/point identities,
        // credentials, request values and upstream bodies must stay out of logs.
        let request = {
            let span = tracing::info_span!("qdrant_encode_request");
            let _entered = span.enter();
            let mut request = self
                .http
                .request(method, self.url.join(path).map_err(|_| failure())?);
            if let Some(body) = body {
                request = request.json(body);
            }
            request
        };
        let mut response = request
            .send()
            .instrument(tracing::info_span!("qdrant_send_request"))
            .await
            .map_err(|_| failure())?;
        tracing::Span::current().record("status", response.status().as_u16());
        if response.status() == StatusCode::NOT_FOUND {
            return Err(AppError::search_not_ready(
                "Qdrant views are missing; rebuild views from silver before searching",
            ));
        }
        if !response.status().is_success() {
            return Err(failure());
        }
        let bytes = async {
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| failure())? {
                if bytes.len().saturating_add(chunk.len()) > limit.min(self.max_response) {
                    return Err(failure());
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        }
        .instrument(tracing::info_span!("qdrant_read_response"))
        .await?;
        tracing::Span::current().record("response_bytes", bytes.len());
        let envelope: Envelope<T> = {
            let span = tracing::info_span!("qdrant_decode_response");
            let _entered = span.enter();
            serde_json::from_slice(&bytes).map_err(|_| failure())?
        };
        if envelope.status != ResponseStatus::Ok {
            return Err(failure());
        }
        envelope.record_metrics();
        Ok(envelope.result)
    }
    /// Inspect an existing collection without repairing or creating anything.
    pub(super) async fn inspect(
        &self,
        name: &str,
        dimensions: Option<usize>,
        memory: MemoryTier,
    ) -> Result<Option<String>, AppError> {
        let info: Info = self
            .call(Method::GET, &format!("collections/{name}"), None::<&()>)
            .await?;
        for &(field, _) in schema_fields(&info, dimensions, memory)? {
            if !info.payload_schema.contains_key(field) {
                return Err(AppError::configuration(
                    "Qdrant views setup is incomplete; rebuild views before querying",
                ));
            }
        }
        Ok(dimensions.and_then(|size| dense_name(&info, size)))
    }

    pub(super) async fn initialize(
        &self,
        name: &str,
        dimensions: Option<usize>,
        policy: MemoryPolicy,
    ) -> Result<Option<String>, AppError> {
        let memory = policy.tier;
        let path = format!("collections/{name}");
        let exists = self
            .http
            .get(self.url.join(&path).map_err(|_| failure())?)
            .send()
            .await
            .map_err(|_| failure())?;
        if exists.status() == StatusCode::NOT_FOUND {
            let vectors = dimensions.map_or_else(BTreeMap::new, |size| {
                BTreeMap::from([(
                    "dense".into(),
                    DenseConfig {
                        size,
                        distance: "Cosine".into(),
                        placement: Placement::new(memory.dense()),
                        quantization_config: None,
                        hnsw_config: None,
                        multivector_config: None,
                        datatype: None,
                    },
                )])
            });
            let sparse_vectors = dimensions.map_or_else(BTreeMap::new, |_| {
                BTreeMap::from([(
                    "lexical".into(),
                    SparseConfig {
                        modifier: "idf".into(),
                        index: Some(Placement::new(memory.index())),
                    },
                )])
            });
            let create = Create {
                vectors,
                sparse_vectors,
                hnsw_config: Hnsw {
                    m: 16,
                    ef_construct: 200,
                    full_scan_threshold: dimensions.map(|_| planner::FULL_SCAN_KIB),
                    placement: Placement::new(memory.index()),
                },
                optimizers_config: dimensions.map(|_| planner::Optimizers::search_default()),
                payload: Placement::new(PAYLOAD_MEMORY),
            };
            // A second API process may create the same collection. Always inspect it afterward.
            let _: Result<bool, _> = self.call(Method::PUT, &path, Some(&create)).await;
        } else if !exists.status().is_success() {
            return Err(failure());
        }
        if policy.activate {
            self.activate_memory(&path, dimensions, memory).await?;
        }
        let info = self.prepare_dense(&path, dimensions, memory).await?;
        let fields = schema_fields(&info, dimensions, memory)?;
        for &(field_name, field_schema) in fields {
            if let Some(installed) = info.payload_schema.get(field_name) {
                if installed.data_type != field_schema {
                    return Err(AppError::configuration(
                        "existing Qdrant payload index has incompatible schema; choose a new group_id",
                    ));
                }
                continue;
            }
            let _: Operation = self
                .call(
                    Method::PUT,
                    &format!("{path}/index?wait=true"),
                    Some(&PayloadIndex {
                        field_name,
                        field_schema,
                    }),
                )
                .await?;
        }
        Ok(dimensions.and_then(|size| dense_name(&info, size)))
    }
    async fn activate_memory(
        &self,
        path: &str,
        dimensions: Option<usize>,
        memory: MemoryTier,
    ) -> Result<(), AppError> {
        #[derive(Serialize)]
        struct PayloadParams {
            payload: Placement,
        }
        #[derive(Serialize)]
        struct SparsePlacement {
            index: Placement,
        }
        #[derive(Serialize)]
        struct Update {
            params: PayloadParams,
            #[serde(skip_serializing_if = "BTreeMap::is_empty")]
            vectors: BTreeMap<String, Placement>,
            #[serde(skip_serializing_if = "BTreeMap::is_empty")]
            sparse_vectors: BTreeMap<String, SparsePlacement>,
            #[serde(skip_serializing_if = "Option::is_none")]
            hnsw_config: Option<Placement>,
        }
        let info: Info = self.call(Method::GET, path, None::<&()>).await?;
        // Validate the current dimension before adding a requested named vector.
        let existing = dimensions
            .map(|_| {
                info.config
                    .params
                    .vectors
                    .get("dense")
                    .map(|v| v.size)
                    .ok_or_else(failure)
            })
            .transpose()?;
        schema_fields_mode(&info, existing, memory, false)?;
        if memory_matches(&info, existing, memory) {
            return Ok(());
        }
        let update = Update {
            params: PayloadParams {
                payload: Placement::new(PAYLOAD_MEMORY),
            },
            vectors: info
                .config
                .params
                .vectors
                .keys()
                .map(|name| (name.clone(), Placement::new(memory.dense())))
                .collect(),
            sparse_vectors: if dimensions.is_some() {
                BTreeMap::from([(
                    "lexical".into(),
                    SparsePlacement {
                        index: Placement::new(memory.index()),
                    },
                )])
            } else {
                BTreeMap::new()
            },
            hnsw_config: dimensions.map(|_| Placement::new(memory.index())),
        };
        let _: bool = self.call(Method::PATCH, path, Some(&update)).await?;
        let info: Info = self.call(Method::GET, path, None::<&()>).await?;
        schema_fields(&info, existing, memory)?;
        Ok(())
    }

    async fn prepare_dense(
        &self,
        path: &str,
        dimensions: Option<usize>,
        memory: MemoryTier,
    ) -> Result<Info, AppError> {
        let mut info: Info = self.call(Method::GET, path, None::<&()>).await?;
        if let Some(size) = dimensions {
            if !(1..=2048).contains(&size) {
                return Err(failure());
            }
            if dense_name(&info, size).is_none() {
                let existing = info
                    .config
                    .params
                    .vectors
                    .get("dense")
                    .ok_or_else(failure)?
                    .size;
                schema_fields(&info, Some(existing), memory)?;
                self.add_dense(path, size, memory).await?;
                info = self.call(Method::GET, path, None::<&()>).await?;
            }
            let name = dense_name(&info, size).ok_or_else(failure)?;
            // Creating a named field and setting cold placement are separate
            // Qdrant operations. Resume only this known secondary field; validate
            // every other schema property before finishing interrupted setup.
            if name != "dense"
                && memory == MemoryTier::Cold
                && info.config.params.vectors[&name]
                    .placement
                    .matches(PhysicalMemory::Cached)
            {
                info.config
                    .params
                    .vectors
                    .get_mut(&name)
                    .ok_or_else(failure)?
                    .placement = Placement::new(memory.dense());
                schema_fields(&info, Some(size), memory)?;
                self.set_dense_memory(path, name, memory).await?;
                info = self.call(Method::GET, path, None::<&()>).await?;
            }
        }
        Ok(info)
    }
    async fn add_dense(&self, path: &str, size: usize, memory: MemoryTier) -> Result<(), AppError> {
        #[derive(Serialize)]
        struct Geometry {
            size: usize,
            distance: &'static str,
        }
        #[derive(Serialize)]
        struct CreateVector {
            dense: Geometry,
        }
        let name = format!("dense_{size}");
        let _: Result<Operation, _> = self
            .call(
                Method::PUT,
                &format!("{path}/vectors/{name}?wait=true"),
                Some(&CreateVector {
                    dense: Geometry {
                        size,
                        distance: "Cosine",
                    },
                }),
            )
            .await;
        if memory == MemoryTier::Cold {
            self.set_dense_memory(path, name, memory).await?;
        }
        Ok(())
    }
    async fn set_dense_memory(
        &self,
        path: &str,
        name: String,
        memory: MemoryTier,
    ) -> Result<(), AppError> {
        #[derive(Serialize)]
        struct Update {
            vectors: BTreeMap<String, Placement>,
        }
        let _: bool = self
            .call(
                Method::PATCH,
                path,
                Some(&Update {
                    vectors: BTreeMap::from([(name, Placement::new(memory.dense()))]),
                }),
            )
            .await?;
        Ok(())
    }
    #[tracing::instrument(name = "qdrant_native_upsert", skip_all, fields(points = input.points.len()))]
    pub(super) async fn upsert<P: Serialize>(
        &self,
        name: &str,
        input: &Upsert<P>,
    ) -> Result<(), AppError> {
        let mut batch = StagingBatch::default();
        for point in &input.points {
            batch
                .push(
                    self,
                    name,
                    Point {
                        id: point.id.clone(),
                        vector: point.vector.clone(),
                        payload: &point.payload,
                    },
                )
                .await?;
        }
        batch.flush(self, name).await
    }
    pub(super) async fn retrieve<P: DeserializeOwned>(
        &self,
        name: &str,
        ids: &[String],
    ) -> Result<Vec<Record<P>>, AppError> {
        self.retrieve_bounded(name, ids, self.max_response).await
    }

    #[tracing::instrument(name = "qdrant_native_retrieve", skip_all, fields(points = ids.len()))]
    pub(super) async fn retrieve_bounded<P: DeserializeOwned>(
        &self,
        name: &str,
        ids: &[String],
        limit: usize,
    ) -> Result<Vec<Record<P>>, AppError> {
        self.call_with_limit(
            Method::POST,
            &format!("collections/{name}/points?consistency=all"),
            Some(&Retrieve {
                ids,
                with_payload: true,
                with_vector: false,
            }),
            limit,
        )
        .await
    }
    #[tracing::instrument(name = "qdrant_native_scroll_page", skip_all, fields(limit))]
    pub(super) async fn scroll_page<P: DeserializeOwned>(
        &self,
        name: &str,
        filter: Filter,
        offset: Option<String>,
        fields: &'static [&'static str],
        limit: usize,
    ) -> Result<ScrollResult<P>, AppError> {
        let reply: ScrollResult<P> = self
            .call(
                Method::POST,
                &format!("collections/{name}/points/scroll?consistency=all"),
                Some(&Scroll {
                    filter,
                    limit,
                    with_payload: Payload::Fields(fields),
                    with_vector: false,
                    offset: offset.clone(),
                }),
            )
            .await?;
        if reply.points.len() > limit
            || (reply.next_page_offset.is_some()
                && (reply.next_page_offset == offset || reply.points.is_empty()))
        {
            return Err(failure());
        }
        Ok(reply)
    }
    #[tracing::instrument(name = "qdrant_native_query", skip_all, fields(limit = input.limit, exact = input.params.exact, dense = matches!(&input.query, QueryVector::Dense(_))))]
    pub(super) async fn query<P: DeserializeOwned>(
        &self,
        name: &str,
        input: &QueryRequest,
    ) -> Result<Vec<Scored<P>>, AppError> {
        let result: QueryResult<P> = self
            .call(
                Method::POST,
                &format!("collections/{name}/points/query?consistency=all"),
                Some(input),
            )
            .await?;
        if result.points.len() > input.limit || result.points.iter().any(|p| !p.score.is_finite()) {
            return Err(failure());
        }
        Ok(result.points)
    }

    pub(super) async fn terms(&self, name: &str, filter: &Filter) -> Result<Vec<String>, AppError> {
        #[derive(Serialize)]
        struct Request<'a> {
            key: &'static str,
            limit: usize,
            exact: bool,
            filter: &'a Filter,
        }
        #[derive(Deserialize)]
        struct Reply {
            hits: Vec<Term>,
        }
        #[derive(Deserialize)]
        struct Term {
            value: String,
            count: u64,
        }
        let reply: Reply = self
            .call(
                Method::POST,
                &format!("collections/{name}/facet?consistency=all"),
                Some(&Request {
                    key: "tokens",
                    limit: crate::views::matching::VOCABULARY_LIMIT + 1,
                    exact: true,
                    filter,
                }),
            )
            .await?;
        if reply.hits.len() > crate::views::matching::VOCABULARY_LIMIT {
            return Err(AppError::usage(
                "lexical matching vocabulary budget exceeded; narrow filters or use match_mode=strict",
            ));
        }
        if reply
            .hits
            .iter()
            .any(|h| h.count == 0 || h.value.is_empty())
        {
            return Err(failure());
        }
        Ok(reply.hits.into_iter().map(|h| h.value).collect())
    }
    #[tracing::instrument(name = "qdrant_native_delete", skip_all)]
    pub(super) async fn delete_filtered(&self, name: &str, filter: Filter) -> Result<(), AppError> {
        self.mutations()?.delete(name, filter).await
    }
}
