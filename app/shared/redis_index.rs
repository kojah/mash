use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};

use super::backend::{ConversationRecord, MessageRecord, conflict, valid_id, valid_provider};
use super::protocol::{
    self, Checkpoint, Event, Forget, Forgotten, Indexed, NodeState, Search, Start, Tombstone,
};
use super::server::Identity;
use super::{Freshness, NodeFreshness};
use crate::storage::{Counts, Message, SearchHit};
use crate::{AppError, semantic};

// Bound uploads and canonical node snapshots to the configured memory budget.
const STAGE_TTL: u32 = 600;
const CANONICAL_CHUNK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CanonicalManifest {
    bytes: usize,
    chunks: usize,
    digest: String,
}

#[derive(Clone)]
pub(super) struct Store {
    connection: ConnectionManager,
    max_node_bytes: usize,
}
#[derive(Clone, Deserialize, Serialize)]
struct Head {
    instance: String,
    generation: String,
    revision: i64,
    version: String,
    counts: Counts,
    last_synced_at: Option<String>,
    last_updated_at: Option<String>,
}
#[derive(Default, Deserialize, Serialize)]
struct Data {
    conversations: BTreeMap<String, ConversationRecord>,
    messages: BTreeMap<String, MessageRecord>,
    sources: BTreeMap<String, Checkpoint>,
    tombstones: BTreeMap<String, Tombstone>,
}
struct Loaded {
    digest: Option<String>,
    head: Option<Head>,
    expected: String,
    data: Data,
}
struct Plan {
    loaded: Loaded,
    revision: i64,
    changed: bool,
    removed: u64,
    purged: u64,
}
fn prefix(group: &str) -> String {
    format!("mash:r:{}:", blake3::hash(group.as_bytes()).to_hex())
}
fn index_name(group: &str) -> String {
    format!("{}index", prefix(group))
}
fn document_prefix(group: &str) -> String {
    format!("{}message:", prefix(group))
}
fn document_key(group: &str, version: &str, id: &str) -> String {
    format!(
        "{}{version}:{}",
        document_prefix(group),
        blake3::hash(id.as_bytes()).to_hex()
    )
}
fn state_key(group: &str, version: &str) -> String {
    format!("{}stage:{version}:state", prefix(group))
}
fn data_key(group: &str, version: &str) -> String {
    format!("{}stage:{version}:data", prefix(group))
}
fn identity_key(provider: &str, id: &str) -> String {
    format!("{provider}\0{id}")
}
fn encode(value: &impl Serialize) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|_| AppError::internal("serialize Redis index data"))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(|_| {
        AppError::shared("invalid Redis index data; restore the index or reindex to a new node_id")
    })
}
fn redis_error(error: redis::RedisError) -> AppError {
    let result = match error.detail().or_else(|| error.code()) {
        Some("MASH_CONFLICT") => conflict("concurrent shared update; retry the operation"),
        Some("MASH_INCOMPATIBLE") => AppError::shared(
            "shared embeddings are incomplete or incompatible; run mash index on publishing nodes",
        ),
        _ => AppError::shared(
            "Redis index operation failed; check service availability, memory, and Redis Search support",
        ),
    };
    drop(error);
    result
}
fn timestamp(value: Option<String>) -> Result<Option<String>, AppError> {
    value
        .map(|value| {
            let (seconds, micros) = value
                .split_once('.')
                .ok_or_else(|| AppError::shared("invalid Redis synchronization timestamp"))?;
            let seconds = seconds
                .parse()
                .map_err(|_| AppError::shared("invalid Redis synchronization timestamp"))?;
            let micros: u32 = micros
                .parse()
                .map_err(|_| AppError::shared("invalid Redis synchronization timestamp"))?;
            if micros >= 1_000_000 {
                return Err(AppError::shared("invalid Redis synchronization timestamp"));
            }
            chrono::DateTime::from_timestamp(seconds, micros * 1000)
                .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
                .ok_or_else(|| AppError::shared("invalid Redis synchronization timestamp"))
        })
        .transpose()
}
impl Store {
    pub(super) async fn connect(
        url: &str,
        groups: &[String],
        dimensions: usize,
        max_node_bytes: usize,
    ) -> Result<Self, AppError> {
        let client = redis::Client::open(url)
            .map_err(|_| AppError::configuration("invalid server Redis connection string"))?;
        let config = redis::aio::ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_secs(5)))
            .set_response_timeout(Some(Duration::from_secs(30)));
        let mut connection = client
            .get_connection_manager_with_config(config)
            .await
            .map_err(redis_error)?;
        for group in groups {
            let dimension_key = format!("{}dimensions", prefix(group));
            let name = index_name(group);
            let exists: Vec<String> = redis::cmd("FT._LIST")
                .query_async(&mut connection)
                .await
                .map_err(redis_error)?;
            let recorded: Option<usize> = redis::cmd("GET")
                .arg(&dimension_key)
                .query_async(&mut connection)
                .await
                .map_err(redis_error)?;
            // Indexes created before dimension metadata existed were always 384.
            if recorded.is_none() && exists.contains(&name) && dimensions != 384 {
                return Err(AppError::configuration(
                    "existing Redis group uses 384 dimensions; choose a new group_id",
                ));
            }
            redis::cmd("SET")
                .arg(&dimension_key)
                .arg(dimensions)
                .arg("NX")
                .query_async::<Option<String>>(&mut connection)
                .await
                .map_err(redis_error)?;
            let installed: usize = redis::cmd("GET")
                .arg(&dimension_key)
                .query_async(&mut connection)
                .await
                .map_err(redis_error)?;
            if installed != dimensions {
                return Err(AppError::configuration(
                    "Redis group uses different embedding dimensions; use a new group_id when changing dimensions",
                ));
            }
            if !exists.contains(&name) {
                let created: Result<(), _> = redis::cmd("FT.CREATE")
                    .arg(&name)
                    .arg("ON")
                    .arg("HASH")
                    .arg("PREFIX")
                    .arg(1)
                    .arg(document_prefix(group))
                    .arg("STOPWORDS")
                    .arg(0)
                    .arg("SCHEMA")
                    .arg("version")
                    .arg("TAG")
                    .arg("conversation")
                    .arg("TAG")
                    .arg("provider_tag")
                    .arg("TAG")
                    .arg("ordinal")
                    .arg("NUMERIC")
                    .arg("SORTABLE")
                    .arg("created_at")
                    .arg("NUMERIC")
                    .arg("searchable")
                    .arg("NUMERIC")
                    .arg("projection")
                    .arg("TEXT")
                    .arg("NOSTEM")
                    .arg("vector")
                    .arg("VECTOR")
                    .arg("FLAT")
                    .arg(6)
                    .arg("TYPE")
                    .arg("FLOAT32")
                    .arg("DIM")
                    .arg(dimensions)
                    .arg("DISTANCE_METRIC")
                    .arg("COSINE")
                    .query_async(&mut connection)
                    .await;
                if created.is_err() {
                    let exists: Vec<String> = redis::cmd("FT._LIST")
                        .query_async(&mut connection)
                        .await
                        .map_err(redis_error)?;
                    if !exists.contains(&name) {
                        created.map_err(redis_error)?;
                    }
                }
            }
        }
        Ok(Self {
            connection,
            max_node_bytes,
        })
    }

    pub(super) fn max_node_bytes(&self) -> u64 {
        u64::try_from(self.max_node_bytes).unwrap_or(u64::MAX)
    }
    async fn head(&self, identity: &Identity) -> Result<(Option<Head>, String), AppError> {
        let raw: Option<String> = redis::cmd("HGET")
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(&identity.node_id)
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        Ok((
            raw.as_deref().map(decode).transpose()?,
            raw.unwrap_or_default(),
        ))
    }
    async fn load(&self, identity: &Identity) -> Result<Loaded, AppError> {
        let (head, expected) = self.head(identity).await?;
        let (data, digest) = if let Some(head) = &head {
            let raw: Option<String> = redis::cmd("GET")
                .arg(data_key(&identity.group_id, &head.version))
                .query_async(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
            let Some(raw) = raw else {
                if self.head(identity).await?.1 != expected {
                    return Err(conflict(
                        "shared node changed while reading; retry the operation",
                    ));
                }
                return Err(AppError::shared(
                    "Redis canonical data is missing; use persistence and noeviction",
                ));
            };
            let manifest: CanonicalManifest = decode(&raw)?;
            if manifest.bytes > self.max_node_bytes
                || manifest.chunks == 0
                || manifest.chunks != manifest.bytes.div_ceil(CANONICAL_CHUNK_BYTES)
            {
                return Err(AppError::shared(
                    "Redis canonical manifest exceeds the configured snapshot limit or is invalid",
                ));
            }
            let mut bytes = Vec::with_capacity(manifest.bytes);
            for chunk in 0..manifest.chunks {
                let part: Option<Vec<u8>> = redis::cmd("GET")
                    .arg(format!(
                        "{}:chunk:{chunk}",
                        data_key(&identity.group_id, &head.version)
                    ))
                    .query_async(&mut self.connection.clone())
                    .await
                    .map_err(redis_error)?;
                let part =
                    part.ok_or_else(|| AppError::shared("Redis canonical chunk is missing"))?;
                if part.len() > CANONICAL_CHUNK_BYTES
                    || bytes.len().saturating_add(part.len()) > manifest.bytes
                {
                    return Err(AppError::shared("invalid Redis canonical chunk length"));
                }
                bytes.extend_from_slice(&part);
            }
            if bytes.len() != manifest.bytes
                || blake3::hash(&bytes).to_hex().as_str() != manifest.digest
            {
                return Err(AppError::shared(
                    "Redis canonical snapshot checksum mismatch",
                ));
            }
            let data = crate::blocking(move || {
                let text = String::from_utf8(bytes)
                    .map_err(|_| AppError::shared("invalid Redis canonical data"))?;
                decode(&text)
            })
            .await?;
            (data, Some(manifest.digest))
        } else {
            (Data::default(), None)
        };
        Ok(Loaded {
            digest,
            head,
            expected,
            data,
        })
    }
    pub(super) async fn status(&self, identity: &Identity) -> Result<Counts, AppError> {
        Ok(self
            .head(identity)
            .await?
            .0
            .map_or_else(Counts::default, |h| h.counts))
    }
    pub(super) async fn freshness(&self, identity: &Identity) -> Result<Freshness, AppError> {
        let heads: HashMap<String, String> = redis::cmd("HGETALL")
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let mut nodes = Vec::new();
        for (node_id, raw) in heads {
            let head: Head = decode(&raw)?;
            nodes.push(NodeFreshness {
                node_id,
                revision: head.revision,
                last_synced_at: timestamp(head.last_synced_at)?,
                last_updated_at: timestamp(head.last_updated_at)?,
                conversations: i64::try_from(head.counts.conversations).unwrap_or(i64::MAX),
                messages: i64::try_from(head.counts.messages).unwrap_or(i64::MAX),
            });
        }
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        Ok(Freshness {
            group_id: identity.group_id.clone(),
            nodes,
        })
    }
    pub(super) async fn state(&self, identity: &Identity) -> Result<NodeState, AppError> {
        let script = "local raw=redis.call('HGET',KEYS[1],ARGV[1]); if not raw then return nil end; local h=cjson.decode(raw); local state=redis.call('GET',ARGV[2]..h.version..':state'); if not state then return redis.error_reply('MASH_STAGE_MISSING') end; return {raw,state}";
        let result: Option<(String, String)> = redis::cmd("EVAL_RO")
            .arg(script)
            .arg(1)
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(&identity.node_id)
            .arg(format!("{}stage:", prefix(&identity.group_id)))
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        if let Some((head, state)) = result {
            let head: Head = decode(&head)?;
            let mut state: NodeState = decode(&state)?;
            state.instance = Some(head.instance);
            state.revision = head.revision;
            if head.generation != identity.generation {
                for source in &mut state.sources {
                    source.ready = false;
                }
            }
            Ok(state)
        } else {
            Ok(NodeState {
                instance: None,
                revision: 0,
                sources: Vec::new(),
                tombstones: Vec::new(),
            })
        }
    }
    pub(super) async fn index(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        if file
            .as_file()
            .metadata()
            .map_err(|_| AppError::internal("inspect index upload"))?
            .len()
            > self.max_node_bytes()
        {
            return Err(AppError::usage(
                "Redis upload exceeds the configured snapshot limit",
            ));
        }
        let loaded = self.load(identity).await?;
        let dimensions = identity.dimensions;
        let plan = crate::blocking(move || plan(&file, loaded, dimensions)).await?;
        let instance = plan
            .loaded
            .head
            .as_ref()
            .map(|h| h.instance.clone())
            .ok_or_else(|| AppError::internal("missing index instance"))?;
        self.commit(identity, plan, instance, true).await
    }
    pub(super) async fn forget(
        &self,
        identity: &Identity,
        input: Forget,
    ) -> Result<Forgotten, AppError> {
        valid_id(&input.instance)?;
        if input.ids.len() > 10000 {
            return Err(AppError::usage("too many forget identities"));
        }
        let loaded = self.load(identity).await?;
        let instance = input.instance.clone();
        let (plan, forgotten) = crate::blocking(move || {
            let accepted = loaded.head.as_ref().map_or(0, |h| h.revision);
            if loaded
                .head
                .as_ref()
                .is_some_and(|h| h.instance != input.instance)
                || input.revision.is_some_and(|r| r <= accepted)
            {
                return Err(conflict("node identity or revision conflict"));
            }
            let revision = input.revision.unwrap_or(
                accepted
                    .checked_add(1)
                    .ok_or_else(|| conflict("node revision exhausted"))?,
            );
            let mut plan = Plan {
                loaded,
                revision,
                changed: false,
                removed: 0,
                purged: 0,
            };
            let mut forgotten = false;
            for id in &input.ids {
                forgotten |= forget(&mut plan.loaded.data, id)?;
            }
            plan.changed = forgotten;
            Ok((plan, forgotten))
        })
        .await?;
        self.commit(identity, plan, instance, false).await?;
        Ok(Forgotten { forgotten })
    }
    #[allow(
        clippy::too_many_lines,
        reason = "bounded document pipelines and stage manifest must finish before the atomic visibility change"
    )]
    async fn stage(
        &self,
        identity: &Identity,
        data: &Data,
        version: &str,
    ) -> Result<Vec<String>, AppError> {
        let json = encode(data)?;
        if json.len() > self.max_node_bytes {
            return Err(AppError::usage(
                "Redis node exceeds the configured snapshot limit",
            ));
        }
        let mut keys = vec![
            data_key(&identity.group_id, version),
            state_key(&identity.group_id, version),
        ];
        let manifest = CanonicalManifest {
            bytes: json.len(),
            chunks: json.len().div_ceil(CANONICAL_CHUNK_BYTES),
            digest: blake3::hash(json.as_bytes()).to_hex().to_string(),
        };
        // Each Redis value remains small even when a node snapshot exceeds 512 MiB.
        for (chunk, bytes) in json.as_bytes().chunks(CANONICAL_CHUNK_BYTES).enumerate() {
            let key = format!("{}:chunk:{chunk}", keys[0]);
            redis::cmd("SET")
                .arg(&key)
                .arg(bytes)
                .arg("EX")
                .arg(STAGE_TTL)
                .query_async::<()>(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
            keys.push(key);
        }
        let state = data.state(identity);
        let mut pipe = redis::pipe();
        pipe.cmd("SET")
            .arg(&keys[0])
            .arg(encode(&manifest)?)
            .arg("EX")
            .arg(STAGE_TTL)
            .ignore();
        pipe.cmd("SET")
            .arg(&keys[1])
            .arg(encode(&state)?)
            .arg("EX")
            .arg(STAGE_TTL)
            .ignore();
        pipe.query_async::<()>(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let mut pipe = redis::pipe();
        let mut queued = 0;
        for message in data.messages.values() {
            let conversation = data
                .conversations
                .get(&message.conversation_id)
                .ok_or_else(|| AppError::internal("orphan Redis message"))?;
            let key = document_key(&identity.group_id, version, &message.id);
            keys.push(key.clone());
            pipe.cmd("HSET")
                .arg(&key)
                .arg("version")
                .arg(version)
                .arg("node_id")
                .arg(&identity.node_id)
                .arg("conversation")
                .arg(
                    blake3::hash(message.conversation_id.as_bytes())
                        .to_hex()
                        .to_string(),
                )
                .arg("provider")
                .arg(&conversation.provider)
                .arg("provider_tag")
                .arg(if conversation.provider == "codex" {
                    "x"
                } else {
                    "c"
                })
                .arg("ordinal")
                .arg(message.ordinal)
                .arg("created_at")
                .arg(message.created_at.unwrap_or(i64::MIN))
                .arg("searchable")
                .arg(u8::from(!message.projection.is_empty()))
                .arg("projection")
                .arg(&message.projection)
                .arg("record")
                .arg(encode(message)?);
            if let Some(vector) = &message.vector {
                pipe.arg("vector").arg(vector_bytes(vector));
            }
            pipe.ignore()
                .cmd("EXPIRE")
                .arg(&key)
                .arg(STAGE_TTL)
                .ignore();
            queued += 1;
            if queued == 128 {
                pipe.query_async::<()>(&mut self.connection.clone())
                    .await
                    .map_err(redis_error)?;
                pipe.clear();
                queued = 0;
            }
        }
        if queued > 0 {
            pipe.query_async::<()>(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
        }
        let manifest = format!("{}stage:{version}:keys", prefix(&identity.group_id));
        for chunk in keys.chunks(128) {
            redis::pipe()
                .cmd("RPUSH")
                .arg(&manifest)
                .arg(chunk)
                .ignore()
                .cmd("EXPIRE")
                .arg(&manifest)
                .arg(STAGE_TTL)
                .ignore()
                .query_async::<()>(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
        }
        keys.push(manifest);
        Ok(keys)
    }
    async fn commit(
        &self,
        identity: &Identity,
        plan: Plan,
        instance: String,
        checked: bool,
    ) -> Result<Indexed, AppError> {
        // Redis Lua cjson emits numbers with 14 significant digits.
        if !(1..=99_999_999_999_999).contains(&plan.revision) {
            return Err(AppError::usage("invalid Redis revision"));
        }
        let counts = plan.loaded.data.counts(identity);
        let mut random = [0u8; 32];
        getrandom::fill(&mut random)
            .map_err(|_| AppError::internal("create Redis staging identity"))?;
        let new_version = blake3::hash(&random).to_hex().to_string();
        let previous = plan.loaded.head.as_ref();
        let reuse = previous.is_some()
            && !plan.changed
            && plan.loaded.digest.as_deref()
                == Some(
                    blake3::hash(encode(&plan.loaded.data)?.as_bytes())
                        .to_hex()
                        .as_str(),
                );
        let version = if reuse {
            previous.unwrap().version.clone()
        } else {
            new_version
        };
        let keys = if reuse {
            vec![
                data_key(&identity.group_id, &version),
                state_key(&identity.group_id, &version),
            ]
        } else {
            self.stage(identity, &plan.loaded.data, &version).await?
        };
        let head = Head {
            instance,
            generation: identity.generation.clone(),
            revision: plan.revision,
            version: version.clone(),
            counts: counts.clone(),
            last_synced_at: previous.and_then(|h| h.last_synced_at.clone()),
            last_updated_at: previous.and_then(|h| h.last_updated_at.clone()),
        };
        let mut command = redis::cmd("EVAL");
        command
            .arg(include_str!("redis/commit.lua"))
            .arg(keys.len() + 1)
            .arg(format!("{}nodes", prefix(&identity.group_id)));
        for key in &keys {
            command.arg(key);
        }
        command
            .arg(&identity.node_id)
            .arg(&plan.loaded.expected)
            .arg(encode(&head)?)
            .arg(u8::from(checked))
            .arg(u8::from(plan.changed))
            .arg(
                previous
                    .filter(|p| p.version != version && !p.version.is_empty())
                    .map_or_else(String::new, |p| {
                        format!("{}stage:{}:keys", prefix(&identity.group_id), p.version)
                    }),
            );
        command
            .query_async::<i64>(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        Ok(Indexed {
            counts,
            revision: plan.revision,
            removed_messages: plan.removed,
            purged_conversations: plan.purged,
        })
    }
    pub(super) async fn search(
        &self,
        identity: &Identity,
        input: Search,
    ) -> Result<protocol::Candidates, AppError> {
        if input.query.trim().is_empty()
            || input.query.len() > 16384
            || !protocol::valid_vector(&input.vector, identity.dimensions)
            || input.generation != identity.generation
        {
            return Err(AppError::usage("invalid search query or vector"));
        }
        valid_provider(input.provider.as_deref())?;
        let words = input
            .query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        let lexical = if words.is_empty() {
            String::new()
        } else {
            format!("@projection:({}) @searchable:[1 1]", words.join(" "))
        };
        let mut filters = String::new();
        if let Some(provider) = input.provider {
            filters.push_str(if provider == "codex" {
                " @provider_tag:{x}"
            } else {
                " @provider_tag:{c}"
            });
        }
        if let Some(days) = input.days {
            use std::fmt::Write;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let cutoff = i64::try_from(now)
                .unwrap_or(i64::MAX)
                .saturating_sub(i64::from(days) * 86400);
            write!(&mut filters, " @created_at:[{cutoff} +inf]")
                .map_err(|_| AppError::internal("format search filter"))?;
        }
        let (lexical, semantic): (redis::Value, redis::Value) = redis::cmd("EVAL_RO")
            .arg(include_str!("redis/search.lua"))
            .arg(2)
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(index_name(&identity.group_id))
            .arg(lexical)
            .arg(filters)
            .arg(vector_bytes(&input.vector))
            .arg(&identity.generation)
            .arg(if input.exclude_self {
                identity.node_id.as_str()
            } else {
                ""
            })
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let (lexical, mut documents) = hits(lexical, true)?;
        let (semantic, other) = hits(semantic, false)?;
        documents.extend(other);
        let documents = documents
            .into_iter()
            .map(|((node_id, id), projection)| protocol::Document {
                node_id,
                id,
                projection,
            })
            .collect();
        Ok(protocol::Candidates {
            lexical,
            semantic,
            documents,
        })
    }
    pub(super) async fn view(
        &self,
        identity: &Identity,
        input: protocol::View,
    ) -> Result<Vec<Message>, AppError> {
        if input.context > 1000 {
            return Err(AppError::usage("invalid view context"));
        }
        valid_id(&input.id)?;
        let value: redis::Value = redis::cmd("EVAL_RO")
            .arg(include_str!("redis/view.lua"))
            .arg(2)
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(index_name(&identity.group_id))
            .arg(input.node_id)
            .arg(document_prefix(&identity.group_id))
            .arg(blake3::hash(input.id.as_bytes()).to_hex().to_string())
            .arg(input.context)
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let redis::Value::Array(values) = value else {
            return Err(AppError::shared("invalid Redis search response"));
        };
        if values.is_empty() {
            return Err(AppError::shared("invalid Redis view response"));
        }
        let mut result = Vec::new();
        for pair in values[1..].chunks(2) {
            if pair.len() != 2 {
                return Err(AppError::shared("invalid Redis view response"));
            }
            let fields: HashMap<String, String> = redis::from_redis_value(pair[1].clone())
                .map_err(|_| AppError::shared("invalid Redis response"))?;
            let message: MessageRecord = decode(
                fields
                    .get("record")
                    .ok_or_else(|| AppError::shared("missing Redis view record"))?,
            )?;
            result.push(Message {
                id: message.id,
                ordinal: message.ordinal,
                role: message.role,
                content: message.content,
                created_at: message.created_at,
            });
        }
        result.sort_by_key(|m| m.ordinal);
        Ok(result)
    }
}
#[allow(clippy::cast_possible_truncation)]
fn vector_bytes(vector: &[f64]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|v| (*v as f32).to_le_bytes())
        .collect()
}
type Documents = HashMap<(String, String), String>;
fn hits(value: redis::Value, lexical: bool) -> Result<(Vec<SearchHit>, Documents), AppError> {
    let redis::Value::Array(values) = value else {
        return Err(AppError::shared("invalid Redis search response"));
    };
    if values.is_empty() {
        return Err(AppError::shared("invalid Redis search response"));
    }
    let mut hits = Vec::new();
    let mut documents = HashMap::new();
    for row in values[1..].chunks(3) {
        if row.len() != 3 {
            return Err(AppError::shared("invalid Redis candidate response"));
        }
        let fields: HashMap<String, String> = redis::from_redis_value(row[2].clone())
            .map_err(|_| AppError::shared("invalid Redis response"))?;
        let message: MessageRecord = decode(
            fields
                .get("record")
                .ok_or_else(|| AppError::shared("missing Redis candidate record"))?,
        )?;
        let node_id = fields
            .get("node_id")
            .ok_or_else(|| AppError::shared("missing Redis candidate owner"))?
            .clone();
        let provider = fields
            .get("provider")
            .ok_or_else(|| AppError::shared("missing Redis candidate provider"))?
            .clone();
        let score: f64 = redis::from_redis_value(row[1].clone())
            .map_err(|_| AppError::shared("invalid Redis response"))?;
        let distance = fields
            .get("distance")
            .map(|v| v.parse::<f32>())
            .transpose()
            .map_err(|_| AppError::shared("invalid Redis vector distance"))?;
        documents.insert((node_id.clone(), message.id.clone()), message.projection);
        hits.push(SearchHit {
            node_id: Some(node_id),
            id: message.id,
            conversation_id: message.conversation_id,
            provider,
            role: message.role,
            content: message.content,
            lexical_score: lexical.then_some(-score),
            semantic_score: distance.map(|d| 1.0 - d),
            fusion_score: 0.0,
            rerank_score: None,
        });
    }
    hits.sort_by(|a, b| {
        if lexical {
            a.lexical_score
                .partial_cmp(&b.lexical_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            b.semantic_score
                .partial_cmp(&a.semantic_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        .then_with(|| a.node_id.cmp(&b.node_id))
        .then_with(|| a.id.cmp(&b.id))
    });
    Ok((hits, documents))
}
impl Data {
    fn counts(&self, identity: &Identity) -> Counts {
        Counts {
            conversations: self.conversations.len() as u64,
            messages: self.messages.len() as u64,
            searchable_messages: self
                .messages
                .values()
                .filter(|m| !m.projection.is_empty())
                .count() as u64,
            embeddings: self
                .messages
                .values()
                .filter(|m| {
                    !m.projection.is_empty()
                        && m.generation.as_deref() == Some(identity.generation.as_str())
                        && m.vector
                            .as_ref()
                            .is_some_and(|v| protocol::valid_vector(v, identity.dimensions))
                })
                .count() as u64,
        }
    }
    fn state(&self, identity: &Identity) -> NodeState {
        let incompatible: HashSet<_> = self
            .messages
            .values()
            .filter(|m| {
                !m.projection.is_empty()
                    && (m.generation.as_deref() != Some(identity.generation.as_str())
                        || m.vector
                            .as_ref()
                            .is_none_or(|v| !protocol::valid_vector(v, identity.dimensions)))
            })
            .map(|m| m.conversation_id.as_str())
            .collect();
        let sources = self
            .sources
            .values()
            .map(|s| {
                let mut s = s.clone();
                s.ready = s
                    .conversation_id
                    .as_deref()
                    .is_none_or(|id| !incompatible.contains(id));
                s
            })
            .collect();
        NodeState {
            instance: None,
            revision: 0,
            sources,
            tombstones: self.tombstones.values().cloned().collect(),
        }
    }
    fn remove(&mut self, id: &str) -> (u64, u64) {
        let before = self.messages.len();
        self.messages.retain(|_, m| m.conversation_id != id);
        (
            (before - self.messages.len()) as u64,
            u64::from(self.conversations.remove(id).is_some()),
        )
    }
}
fn forget(data: &mut Data, id: &str) -> Result<bool, AppError> {
    valid_id(id)?;
    let providers = data.conversations.get(id).map_or_else(
        || vec!["claude-code".to_owned(), "codex".to_owned()],
        |c| vec![c.provider.clone()],
    );
    let (_, removed) = data.remove(id);
    for provider in providers {
        data.tombstones.insert(
            identity_key(&provider, id),
            Tombstone {
                provider,
                conversation_id: id.to_owned(),
            },
        );
    }
    Ok(removed > 0)
}
fn start(loaded: &mut Loaded, input: &Start) -> Result<(i64, bool), AppError> {
    valid_id(&input.instance)?;
    if input.revision.is_some_and(|r| r < 1)
        || input.base_revision.is_some_and(|r| r < 0)
        || input.forgets.len() > 10000
        || (input.snapshot && (input.base_revision.is_some() || !input.forgets.is_empty()))
    {
        return Err(AppError::usage("invalid index metadata"));
    }
    if loaded
        .head
        .as_ref()
        .is_some_and(|h| h.instance != input.instance)
    {
        return Err(conflict(
            "node_id belongs to another database or indexing mode",
        ));
    }
    let accepted = loaded.head.as_ref().map_or(0, |h| h.revision);
    let revision = if input.snapshot {
        let revision = input
            .revision
            .ok_or_else(|| AppError::usage("snapshot requires revision"))?;
        if revision < accepted {
            return Err(conflict("shared index has a newer snapshot"));
        }
        revision
    } else {
        if input.base_revision != Some(accepted) || input.revision.is_some_and(|r| r <= accepted) {
            return Err(conflict(
                "shared index changed while preparing upload; retry indexing",
            ));
        }
        input.revision.unwrap_or(
            accepted
                .checked_add(1)
                .ok_or_else(|| conflict("node revision exhausted"))?,
        )
    };
    if loaded.head.is_none() {
        loaded.head = Some(Head {
            instance: input.instance.clone(),
            generation: semantic::SHARED_EMBEDDING_GENERATION.to_owned(),
            revision: 0,
            version: String::new(),
            counts: Counts::default(),
            last_synced_at: None,
            last_updated_at: None,
        });
    }
    Ok((revision, input.snapshot && revision == accepted))
}
#[allow(
    clippy::too_many_lines,
    reason = "validate the complete upload before staging any Redis data"
)]
fn plan(
    file: &tempfile::NamedTempFile,
    mut loaded: Loaded,
    dimensions: usize,
) -> Result<Plan, AppError> {
    use std::io::BufRead;
    let mut lines = std::io::BufReader::new(
        file.reopen()
            .map_err(|_| AppError::internal("open incoming index"))?,
    )
    .lines();
    let first = lines
        .next()
        .ok_or_else(|| AppError::usage("empty index stream"))?
        .map_err(|_| AppError::usage("invalid index stream"))?;
    let Event::Start(input) =
        serde_json::from_str(&first).map_err(|_| AppError::usage("invalid index start"))?
    else {
        return Err(AppError::usage("index stream must start with metadata"));
    };
    let (revision, duplicate) = start(&mut loaded, &input)?;
    let mut plan = Plan {
        loaded,
        revision,
        changed: false,
        removed: 0,
        purged: 0,
    };
    if !duplicate {
        for id in &input.forgets {
            plan.changed |= forget(&mut plan.loaded.data, id)?;
        }
        if input.snapshot {
            plan.loaded.data.conversations.clear();
            plan.loaded.data.messages.clear();
            plan.loaded.data.sources.clear();
            plan.changed = true;
        }
    }
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();
    let mut reconciled = HashSet::new();
    let mut finished = false;
    for line in lines {
        if finished {
            return Err(AppError::usage("data after index finish"));
        }
        let event: Event =
            serde_json::from_str(&line.map_err(|_| AppError::usage("invalid index stream"))?)
                .map_err(|_| AppError::usage("invalid index event"))?;
        if matches!(event, Event::Finish) {
            finished = true;
            continue;
        }
        if matches!(event, Event::Start(_)) {
            return Err(AppError::usage("duplicate index start"));
        }
        if duplicate {
            continue;
        }
        let data = &mut plan.loaded.data;
        match event {
            Event::Batch { kind, records } => {
                if records.len() > 128 {
                    return Err(AppError::usage("index batch exceeds 128 records"));
                }
                for value in records {
                    match kind.as_str() {
                        "tombstones" => {
                            let t: Tombstone = serde_json::from_value(value)
                                .map_err(|_| AppError::usage("invalid tombstone"))?;
                            valid_provider(Some(&t.provider))?;
                            valid_id(&t.conversation_id)?;
                            if data
                                .conversations
                                .get(&t.conversation_id)
                                .is_some_and(|c| c.provider == t.provider)
                            {
                                data.remove(&t.conversation_id);
                            }
                            data.tombstones
                                .insert(identity_key(&t.provider, &t.conversation_id), t);
                            plan.changed = true;
                        }
                        "conversations" => {
                            let c: ConversationRecord = serde_json::from_value(value)
                                .map_err(|_| AppError::usage("invalid conversation"))?;
                            valid_provider(Some(&c.provider))?;
                            valid_id(&c.id)?;
                            if !input.snapshot {
                                data.remove(&c.id);
                            } else if data.conversations.contains_key(&c.id) {
                                return Err(AppError::usage("duplicate conversation"));
                            }
                            if !data
                                .tombstones
                                .contains_key(&identity_key(&c.provider, &c.id))
                            {
                                data.conversations.insert(c.id.clone(), c);
                                plan.changed = true;
                            }
                        }
                        "messages" => {
                            let m: MessageRecord = serde_json::from_value(value)
                                .map_err(|_| AppError::usage("invalid message"))?;
                            valid_id(&m.id)?;
                            valid_id(&m.conversation_id)?;
                            if m.ordinal < 0
                                || m.vector
                                    .as_ref()
                                    .is_some_and(|v| !protocol::valid_vector(v, dimensions))
                                || (m.projection.is_empty()
                                    && (m.vector.is_some() || m.generation.is_some()))
                            {
                                return Err(AppError::usage(
                                    "invalid message embedding or ordinal",
                                ));
                            }
                            if data.conversations.contains_key(&m.conversation_id) {
                                if data.messages.insert(m.id.clone(), m).is_some() {
                                    return Err(AppError::usage("duplicate message"));
                                }
                                plan.changed = true;
                            }
                        }
                        _ => return Err(AppError::usage("unknown index batch kind")),
                    }
                }
            }
            Event::Source(s) if !input.snapshot => {
                valid_provider(Some(&s.provider))?;
                valid_id(&s.path_hash)?;
                if s.size_bytes < 0 || reconciled.contains(&s.provider) {
                    return Err(AppError::usage("invalid source checkpoint"));
                }
                data.sources
                    .insert(identity_key(&s.provider, &s.path_hash), s);
            }
            Event::Seen { provider, hashes } if !input.snapshot => {
                valid_provider(Some(&provider))?;
                if hashes.len() > 128 || reconciled.contains(&provider) {
                    return Err(AppError::usage("invalid observed sources"));
                }
                seen.entry(provider).or_default().extend(hashes);
            }
            Event::Reconcile { provider, cutoff } if !input.snapshot => {
                valid_provider(Some(&provider))?;
                if !reconciled.insert(provider.clone()) {
                    return Err(AppError::usage("duplicate reconciliation"));
                }
                let observed = seen.get(&provider);
                let mut missing = Vec::new();
                data.sources.retain(|_, s| {
                    let remove = s.provider == provider
                        && observed.is_none_or(|seen| !seen.contains(&s.path_hash))
                        && cutoff.is_none_or(|c| s.modified_ns >= c);
                    if remove {
                        missing.extend(s.conversation_id.clone());
                    }
                    !remove
                });
                for id in missing {
                    if !data
                        .sources
                        .values()
                        .any(|s| s.conversation_id.as_deref() == Some(&id))
                    {
                        let (m, c) = data.remove(&id);
                        plan.removed += m;
                        plan.purged += c;
                        plan.changed |= c > 0;
                    }
                }
            }
            Event::Remove { id } if !input.snapshot => {
                valid_id(&id)?;
                let (m, c) = data.remove(&id);
                plan.removed += m;
                plan.purged += c;
                plan.changed |= c > 0;
            }
            _ => {
                return Err(AppError::usage(
                    "invalid index event for this indexing mode",
                ));
            }
        }
    }
    if !finished {
        return Err(AppError::usage("incomplete index stream"));
    }
    let mut ordinals = HashSet::new();
    if plan
        .loaded
        .data
        .messages
        .values()
        .any(|m| !ordinals.insert((&m.conversation_id, m.ordinal)))
    {
        return Err(AppError::usage("duplicate message ordinal"));
    }
    Ok(plan)
}
