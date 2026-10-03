//! Small typed REST boundary; credentials and upstream bodies never enter errors.
use super::{AppError, MemoryTier, failure};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::time::Duration;
#[cfg(test)]
mod schema_tests;

#[derive(Clone)]
pub(super) struct Client {
    http: reqwest::Client,
    url: reqwest::Url,
    max_response: usize,
}
#[derive(Deserialize)]
struct Envelope<T> {
    result: T,
    status: ResponseStatus,
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
#[derive(Default, Serialize)]
pub(super) struct Vectors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dense: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical: Option<Sparse>,
}
#[derive(Serialize)]
pub(super) struct Sparse {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
}
#[derive(Serialize)]
pub(super) struct Upsert<P> {
    pub points: Vec<Point<P>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_filter: Option<Filter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_mode: Option<UpdateMode>,
}
#[derive(Serialize)]
struct UpsertSlice<'a, P> {
    points: &'a [Point<P>],
    #[serde(skip_serializing_if = "Option::is_none")]
    update_filter: Option<&'a Filter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    update_mode: Option<&'a UpdateMode>,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum UpdateMode {
    InsertOnly,
    UpdateOnly,
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
    IntegerMatch { key: String, r#match: IntegerMatch },
    Range { key: String, range: Range },
    Nested(Filter),
}
#[derive(Clone, Serialize)]
pub(super) struct Match {
    pub any: Vec<String>,
}
#[derive(Clone, Serialize)]
pub(super) struct IntegerMatch {
    pub any: Vec<i64>,
}
pub(super) fn integers(key: &str, any: Vec<i64>) -> Condition {
    Condition::IntegerMatch {
        key: key.into(),
        r#match: IntegerMatch { any },
    }
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
    pub using: &'static str,
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
    All(bool),
    Fields(&'static [&'static str]),
}
#[derive(Deserialize)]
pub(super) struct ScrollResult<P> {
    pub points: Vec<Record<P>>,
    pub next_page_offset: Option<String>,
}
#[derive(Serialize)]
struct Delete {
    filter: Filter,
}
#[derive(Deserialize)]
struct Operation {
    status: OperationStatus,
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
    #[serde(flatten)]
    placement: Placement,
}

#[derive(Serialize, Deserialize)]
struct Placement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory: Option<MemoryTier>,
    // Qdrant may still report this deprecated field; explicit memory takes precedence.
    #[serde(default, skip_serializing)]
    on_disk: Option<bool>,
}

impl Placement {
    fn new(memory: MemoryTier) -> Self {
        Self {
            memory: Some(memory),
            on_disk: None,
        }
    }

    fn matches(&self, expected: MemoryTier) -> bool {
        self.memory.unwrap_or(if self.on_disk == Some(true) {
            MemoryTier::Cold
        } else {
            MemoryTier::Cached
        }) == expected
    }
}
#[derive(Serialize)]
struct Create {
    vectors: BTreeMap<String, DenseConfig>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    sparse_vectors: BTreeMap<String, SparseConfig>,
    hnsw_config: Hnsw,
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
    #[serde(default)]
    quantization_config: Option<serde::de::IgnoredAny>,
}
#[derive(Deserialize)]
struct Parameters {
    vectors: BTreeMap<String, DenseConfig>,
    #[serde(default)]
    sparse_vectors: Option<BTreeMap<String, SparseConfig>>,
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
    let vectors = &info.config.params.vectors;
    let valid = if let Some(dimensions) = dimensions {
        vectors.len() == 1
            && vectors.get("dense").is_some_and(|v| {
                v.size == dimensions
                    && v.placement.matches(memory)
                    && v.distance == "Cosine"
                    && v.quantization_config.is_none()
                    && v.hnsw_config.is_none()
                    && v.multivector_config.is_none()
                    && v.datatype.as_deref().is_none_or(|t| t == "float32")
            })
            && info.config.params.sparse_vectors.as_ref().is_some_and(|s| {
                s.len() == 1
                    && s.get("lexical").is_some_and(|v| {
                        v.modifier == "idf"
                            && v.index
                                .as_ref()
                                .map_or(memory == MemoryTier::Cached, |p| p.matches(memory))
                    })
            })
            && info.config.hnsw_config.m == 16
            && info.config.hnsw_config.ef_construct == 200
            && info.config.hnsw_config.placement.matches(memory)
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
    if !valid {
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
            ("conversation_id", "keyword"),
            ("project", "keyword"),
            ("repository", "keyword"),
            ("tokens", "keyword"),
            ("created_at", "integer"),
            ("message.ordinal", "integer"),
            ("message.context.ordinal", "integer"),
        ]
    } else {
        &[("kind", "keyword"), ("version", "keyword")]
    };
    Ok(fields)
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
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| AppError::configuration("could not configure Qdrant HTTP client"))?;
        Ok(Self {
            http,
            url,
            max_response: max_node_bytes.saturating_add(16 * 1024 * 1024),
        })
    }
    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T, AppError> {
        let mut request = self
            .http
            .request(method, self.url.join(path).map_err(|_| failure())?);
        if let Some(body) = body {
            request = request.json(body);
        }
        let mut response = request.send().await.map_err(|_| failure())?;
        if !response.status().is_success() {
            return Err(failure());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| failure())? {
            if bytes.len().saturating_add(chunk.len()) > self.max_response {
                return Err(failure());
            }
            bytes.extend_from_slice(&chunk);
        }
        let envelope: Envelope<T> = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        if envelope.status != ResponseStatus::Ok {
            return Err(failure());
        }
        Ok(envelope.result)
    }
    pub(super) async fn initialize(
        &self,
        name: &str,
        dimensions: Option<usize>,
        memory: MemoryTier,
    ) -> Result<(), AppError> {
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
                        placement: Placement::new(memory),
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
                        index: Some(Placement::new(memory)),
                    },
                )])
            });
            let create = Create {
                vectors,
                sparse_vectors,
                hnsw_config: Hnsw {
                    m: 16,
                    ef_construct: 200,
                    placement: Placement::new(memory),
                },
            };
            // A second API process may create the same collection. Always inspect it afterward.
            let _: Result<bool, _> = self.call(Method::PUT, &path, Some(&create)).await;
        } else if !exists.status().is_success() {
            return Err(failure());
        }
        let info: Info = self.call(Method::GET, &path, None::<&()>).await?;
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
        Ok(())
    }
    pub(super) async fn upsert<P: Serialize>(
        &self,
        name: &str,
        input: &Upsert<P>,
    ) -> Result<(), AppError> {
        // The canonical node limit may exceed Qdrant's default 32 MiB request
        // limit. Split by encoded bytes as well as the caller's record count.
        const BUDGET: usize = 30 * 1024 * 1024;
        let mut start = 0;
        let mut bytes = 1024;
        for (index, point) in input.points.iter().enumerate() {
            let size = serde_json::to_vec(point).map_err(|_| failure())?.len();
            if size > BUDGET - 1024 {
                return Err(AppError::usage(
                    "Qdrant document exceeds the request-size limit",
                ));
            }
            if bytes + size > BUDGET {
                self.send_points(name, input, start, index).await?;
                start = index;
                bytes = 1024;
            }
            bytes += size + 1;
        }
        if start < input.points.len() {
            self.send_points(name, input, start, input.points.len())
                .await?;
        }
        Ok(())
    }
    async fn send_points<P: Serialize>(
        &self,
        name: &str,
        input: &Upsert<P>,
        start: usize,
        end: usize,
    ) -> Result<(), AppError> {
        let body = UpsertSlice {
            points: &input.points[start..end],
            update_filter: input.update_filter.as_ref(),
            update_mode: input.update_mode.as_ref(),
        };
        let op: Operation = self
            .call(
                Method::PUT,
                &format!("collections/{name}/points?wait=true&ordering=strong"),
                Some(&body),
            )
            .await?;
        if op.status != OperationStatus::Completed {
            return Err(failure());
        }
        Ok(())
    }
    pub(super) async fn retrieve<P: DeserializeOwned>(
        &self,
        name: &str,
        ids: &[String],
    ) -> Result<Vec<Record<P>>, AppError> {
        self.call(
            Method::POST,
            &format!("collections/{name}/points?consistency=all"),
            Some(&Retrieve {
                ids,
                with_payload: true,
                with_vector: false,
            }),
        )
        .await
    }
    pub(super) async fn scroll<P: DeserializeOwned>(
        &self,
        name: &str,
        filter: Filter,
    ) -> Result<Vec<Record<P>>, AppError> {
        self.scroll_selected(name, filter, Payload::All(true), 100)
            .await
    }
    pub(super) async fn scroll_fields<P: DeserializeOwned>(
        &self,
        name: &str,
        filter: Filter,
        fields: &'static [&'static str],
    ) -> Result<Vec<Record<P>>, AppError> {
        // Context outlines contain only identities and topology, not message bodies.
        // Larger pages avoid serial round trips for long, parent-linked threads.
        self.scroll_selected(name, filter, Payload::Fields(fields), 1000)
            .await
    }
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
    async fn scroll_selected<P: DeserializeOwned>(
        &self,
        name: &str,
        filter: Filter,
        payload: Payload,
        limit: usize,
    ) -> Result<Vec<Record<P>>, AppError> {
        let mut output = Vec::new();
        let mut offset = None;
        loop {
            let reply: ScrollResult<P> = self
                .call(
                    Method::POST,
                    &format!("collections/{name}/points/scroll?consistency=all"),
                    Some(&Scroll {
                        filter: filter.clone(),
                        limit,
                        with_payload: payload.clone(),
                        with_vector: false,
                        offset: offset.clone(),
                    }),
                )
                .await?;
            if reply.points.len() > limit
                || (reply.next_page_offset.is_some() && reply.points.is_empty())
            {
                return Err(failure());
            }
            output.extend(reply.points);
            if reply.next_page_offset.is_none() {
                return Ok(output);
            }
            if reply.next_page_offset == offset {
                return Err(failure());
            }
            offset = reply.next_page_offset;
        }
    }
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
    pub(super) async fn delete_version(&self, name: &str, version: &str) -> Result<(), AppError> {
        let op: Operation = self
            .call(
                Method::POST,
                &format!("collections/{name}/points/delete?wait=true&ordering=strong"),
                Some(&Delete {
                    filter: Filter {
                        must: vec![matches("version", vec![version.into()])],
                        ..Filter::default()
                    },
                }),
            )
            .await?;
        if op.status != OperationStatus::Completed {
            return Err(failure());
        }
        Ok(())
    }
}
