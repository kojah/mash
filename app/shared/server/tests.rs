use super::*;
use serde_json::{Value, json};

mod antigravity;
mod context;
mod correlation;
mod cursor;
mod native;
mod selections;
mod windsurf;

const FIRST: &str = "test-api-token-first-32-bytes-long";
const SECOND: &str = "test-api-token-second-32-bytes-long";
const OTHER: &str = "test-api-token-other-32-bytes-long";

fn provider_snapshot(vector: &[f64], revision: i64) -> Vec<Value> {
    use crate::provider::Provider;
    use crate::shared::protocol::{Batch, ConversationRecord, Event, MessageRecord, Start};
    let conversations = Provider::ALL
        .iter()
        .map(|provider| ConversationRecord {
            id: provider.as_str().into(),
            provider: *provider,
            title: None,
            project: None,
            repository: None,
            created_at: None,
            updated_at: None,
        })
        .collect();
    let messages = Provider::ALL
        .iter()
        .map(|provider| MessageRecord {
            id: format!("{provider}-message"),
            conversation_id: provider.as_str().into(),
            ordinal: 0,
            context: crate::message_context::MessageContext::Linear,
            role: crate::role::Role::User,
            content: "providerneedle canonical history".into(),
            projection: "providerneedle canonical history".into(),
            created_at: None,
            generation: Some(crate::semantic::SHARED_EMBEDDING_GENERATION.into()),
            vector: Some(vector.to_vec()),
        })
        .collect();
    [
        Event::Start(Start {
            instance: "provider-contract".into(),
            revision: Some(revision),
            base_revision: None,
            snapshot: true,
            forgets: Vec::new(),
        }),
        Event::Batch(Batch::Conversations {
            records: conversations,
        }),
        Event::Batch(Batch::Messages { records: messages }),
        Event::Finish,
    ]
    .into_iter()
    .map(|event| serde_json::to_value(event).unwrap())
    .collect()
}

async fn provider_contract(service: Service) {
    use crate::provider::Provider;
    use crate::shared::protocol::{Candidates, Search, View};
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    assert_eq!(
        service
            .upload(FIRST, &provider_snapshot(&vector, 1))
            .await
            .status(),
        StatusCode::OK
    );
    for &provider in Provider::ALL {
        for direct in [true, false] {
            let input = Search {
                unit: crate::search_results::SearchUnit::Messages,
                query: "providerneedle".into(),
                generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
                vector: vector.clone(),
                provider: direct.then_some(provider),
                days: None,
                exclude_self: false,
                filter: (!direct).then(|| crate::search_filter::Filter::Provider(vec![provider])),
            };
            let result: Candidates = service
                .request("search", FIRST)
                .json(&input)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(result.lexical.len(), 1, "lexical {provider}");
            assert_eq!(result.semantic.len(), 1, "semantic {provider}");
            assert_eq!(result.lexical[0].provider, provider);
            assert_eq!(result.semantic[0].provider, provider);
        }
        let input = View {
            node_id: "first".into(),
            id: format!("{provider}-message"),
            context: 0,
            content: crate::storage::MessageContent::Canonical,
        };
        let result: Vec<crate::storage::Message> = service
            .request("view", FIRST)
            .json(&input)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(result[0].content, "providerneedle canonical history");
    }
    selections::verify_thread_provider_scopes(&service, &vector).await;
    verify_delayed_provider_publication_stays_forgotten(&service, &vector).await;
    service.cleanup().await;
}

async fn verify_delayed_provider_publication_stays_forgotten(service: &Service, vector: &[f64]) {
    use crate::shared::protocol::{Forget, Forgotten, Indexed};
    let input = Forget {
        instance: "provider-contract".into(),
        revision: Some(1),
        ids: crate::provider::Provider::ALL
            .iter()
            .map(|provider| provider.as_str().to_owned())
            .collect(),
    };
    let forgotten: Forgotten = service
        .request("forget", SECOND)
        .json(&input)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!forgotten.forgotten);
    let indexed: Indexed = service
        .upload(SECOND, &provider_snapshot(vector, 2))
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(indexed.counts.messages, 0);
    assert_eq!(indexed.counts.conversations, 0);
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL with pgvector"]
async fn postgres_round_trips_every_supported_provider_with_filters() {
    provider_contract(Service::new_backend(true, false).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_round_trips_every_supported_provider_with_filters() {
    provider_contract(Service::new_backend(false, true).await).await;
}

async fn wal_checkpoint_contract(service: Service) {
    use crate::shared::protocol::{Checkpoint, Event, NodeState, Start};
    let hash = blake3::hash(b"database-session-alpha").to_hex().to_string();
    let fingerprint = blake3::hash(b"database-and-wal-stamp").to_hex().to_string();
    let source = Checkpoint {
        provider: crate::provider::Provider::Hermes,
        path_hash: hash.clone(),
        size_bytes: 8192,
        modified_ns: 200,
        fingerprint: fingerprint.clone(),
        conversation_id: None,
        ready: true,
    };
    let events = [
        Event::Start(Start {
            instance: "native-session-test".into(),
            revision: None,
            base_revision: Some(0),
            snapshot: false,
            forgets: Vec::new(),
        }),
        Event::Source(source),
        Event::Finish,
    ]
    .into_iter()
    .map(|event| serde_json::to_value(event).unwrap())
    .collect::<Vec<_>>();
    service
        .upload(FIRST, &events)
        .await
        .error_for_status()
        .unwrap();
    let state: NodeState = serde_json::from_value(service.get("state", FIRST).await).unwrap();
    assert_eq!(state.sources.len(), 1);
    assert_eq!(state.sources[0].provider, crate::provider::Provider::Hermes);
    assert_eq!(state.sources[0].path_hash, hash);
    assert_eq!(state.sources[0].fingerprint, fingerprint);
    assert_eq!(state.sources[0].modified_ns, 200);
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL with pgvector"]
async fn postgres_round_trips_native_session_wal_fingerprints() {
    wal_checkpoint_contract(Service::new_backend(true, false).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_round_trips_native_session_wal_fingerprints() {
    wal_checkpoint_contract(Service::new_backend(false, true).await).await;
}

async fn remote_space_contract(service: Service) {
    let generation = crate::semantic::model_space("voyage-4", 1024);
    let mut events = snapshot(1);
    for (i, record) in events[2]["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
        .take(2)
    {
        let mut vector = vec![0.0; 1024];
        vector[0] = if i == 0 { 127.0 } else { -127.0 };
        record["vector"] = json!(vector);
        record["generation"] = json!(generation);
    }
    let upload = service.upload(FIRST, &events).await;
    assert_eq!(
        upload.status(),
        StatusCode::OK,
        "{}",
        upload.text().await.unwrap()
    );
    assert_eq!(service.get("status", FIRST).await["embeddings"], 2);
    let mut vector = vec![0.0; 1024];
    vector[0] = 127.0;
    let mut query = json!({"query":"hello", "vector":vector, "generation":generation});
    let result: Value = service
        .request("search", FIRST)
        .json(&query)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["semantic"][0]["id"], "searchable");
    query["generation"] = json!(crate::semantic::SHARED_EMBEDDING_GENERATION);
    assert_eq!(
        service
            .request("search", FIRST)
            .json(&query)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let before = service.get("freshness", FIRST).await;
    events[0]["revision"] = json!(2);
    events[2]["records"][0]["vector"] = json!([127.0]);
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(service.get("freshness", FIRST).await, before);
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL with pgvector"]
async fn postgres_publication_remote_model_space_supports_1024_dimensions() {
    remote_space_contract(Service::with_dimensions(true, false, 1024).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_publication_remote_model_space_supports_1024_dimensions() {
    remote_space_contract(Service::with_dimensions(false, true, 1024).await).await;
}

struct Service {
    pool: Pool,
    store: Store,
    url: String,
    group: String,
    task: tokio::task::JoinHandle<()>,
}

async fn exclude_own_contract(service: Service) {
    let mut own = snapshot(1);
    let template = own[2]["records"][0].clone();
    for i in 0..60 {
        let mut record = template.clone();
        record["id"] = json!(format!("own-{i}"));
        record["ordinal"] = json!(i + 3);
        own[2]["records"].as_array_mut().unwrap().push(record);
    }
    assert_eq!(service.upload(FIRST, &own).await.status(), StatusCode::OK);
    assert_eq!(
        service.upload(SECOND, &snapshot(1)).await.status(),
        StatusCode::OK
    );
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    let query = json!({"query":"hello", "vector":vector, "exclude_self":true});
    let found: Value = service
        .request("search", FIRST)
        .json(&query)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(found["lexical"].as_array().unwrap().len(), 1);
    assert_eq!(found["semantic"].as_array().unwrap().len(), 2);
    for hit in found["lexical"]
        .as_array()
        .unwrap()
        .iter()
        .chain(found["semantic"].as_array().unwrap())
    {
        assert_eq!(hit["node_id"], "second");
    }
    // Own incomplete embeddings must not block searches of other nodes.
    own[0]["revision"] = json!(2);
    own[2]["records"][0]["vector"] = Value::Null;
    own[2]["records"][0]["generation"] = Value::Null;
    assert_eq!(service.upload(FIRST, &own).await.status(), StatusCode::OK);
    assert_eq!(
        service
            .request("search", FIRST)
            .json(&query)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let mut query = query;
    query["exclude_self"] = json!(false);
    assert!(
        !service
            .request("search", FIRST)
            .json(&query)
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    service.cleanup().await;
}

async fn rich_filter_contract(service: Service) {
    let mut first = snapshot(1);
    let repository = "ssh://git/example/o'reilly\\repo";
    first[1]["records"][0]["project"] = json!("/work/alpha");
    first[1]["records"][0]["repository"] = json!(repository);
    first[2]["records"][0]["created_at"] = json!(1000);
    first[2]["records"][1]["created_at"] = json!(2000);
    first[2]["records"][1]["role"] = json!("assistant");
    first[2]["records"][1]["projection"] = json!("hello assistant");
    first[2]["records"][2]["created_at"] = Value::Null;
    let template = first[2]["records"][0].clone();
    for i in 0..60 {
        let mut record = template.clone();
        record["id"] = json!(format!("noise-{i}"));
        record["ordinal"] = json!(i + 3);
        first[2]["records"].as_array_mut().unwrap().push(record);
    }
    assert_eq!(service.upload(FIRST, &first).await.status(), StatusCode::OK);
    assert_eq!(
        service.upload(SECOND, &snapshot(1)).await.status(),
        StatusCode::OK
    );
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    let filter = json!({"all":[{"node":["first"]},{"provider":["codex"]},{"role":["assistant"]},{"conversation":["conversation"]},{"project":["/work/alpha"]},{"repository":[repository]},{"after":2},{"before":3}]});
    let query = json!({"query":"hello", "vector":vector,"filter":filter});
    let found: Value = service
        .request("search", FIRST)
        .json(&query)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    for kind in ["lexical", "semantic"] {
        assert_eq!(found[kind].as_array().unwrap().len(), 1, "{found}");
        assert_eq!(found[kind][0]["id"], "opposite");
    }
    for filter in [
        json!({"not":{"project":["/work/alpha"]}}),
        json!({"any":[{"node":["second"]},{"role":["nonexistent"]}]}),
    ] {
        let mut query = query.clone();
        query["filter"] = filter;
        let found: Value = service
            .request("search", FIRST)
            .json(&query)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(found["semantic"].as_array().unwrap().len(), 2, "{found}");
        assert!(
            found["semantic"]
                .as_array()
                .unwrap()
                .iter()
                .all(|h| h["node_id"] == "second")
        );
    }
    let mut invalid = query;
    invalid["filter"] = json!({"provider":["unsupported"]});
    assert_eq!(
        service
            .request("search", FIRST)
            .json(&invalid)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL with pgvector"]
async fn postgres_publication_rich_filters_apply_before_ranking() {
    rich_filter_contract(Service::new(true).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_publication_rich_filters_apply_before_ranking() {
    rich_filter_contract(Service::with_dimensions(false, true, 384).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_repairs_old_metadata_without_changing_snapshot_freshness() {
    let service = Service::with_dimensions(false, true, 384).await;
    assert_eq!(
        service.upload(FIRST, &snapshot(1)).await.status(),
        StatusCode::OK
    );
    let before = service.get("freshness", FIRST).await;
    let url = std::env::var("MASH_TEST_REDIS_URL").unwrap();
    let mut connection = redis::Client::open(url)
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let pattern = format!(
        "mash:r:{}:message:*",
        blake3::hash(service.group.as_bytes()).to_hex()
    );
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(pattern)
        .query_async(&mut connection)
        .await
        .unwrap();
    assert_eq!(keys.len(), 3);
    let heads = format!(
        "mash:r:{}:nodes",
        blake3::hash(service.group.as_bytes()).to_hex()
    );
    redis::cmd("EVAL").arg("local h=cjson.decode(redis.call('HGET',KEYS[1],ARGV[1])); h.metadata_ready=nil; redis.call('HSET',KEYS[1],ARGV[1],cjson.encode(h))").arg(1).arg(heads).arg("first").query_async::<()>(&mut connection).await.unwrap();
    for key in &keys {
        redis::cmd("HDEL")
            .arg(key)
            .arg("conversation_record")
            .query_async::<i64>(&mut connection)
            .await
            .unwrap();
    }
    let Store::Redis(store) = &service.store else {
        panic!("Redis fixture");
    };
    let identity = Identity {
        group_id: service.group.clone(),
        node_id: "first".into(),
        generation: crate::semantic::SHARED_EMBEDDING_GENERATION.into(),
        dimensions: 384,
    };
    store.repair_metadata(&identity).await.unwrap();
    store.repair_metadata(&identity).await.unwrap(); // Idempotent.
    for key in &keys {
        let raw: String = redis::cmd("HGET")
            .arg(key)
            .arg("conversation_record")
            .query_async(&mut connection)
            .await
            .unwrap();
        let conversation: protocol::ConversationRecord = serde_json::from_str(&raw).unwrap();
        assert_eq!(conversation.project.as_deref(), Some("/work/mash"));
        assert_eq!(conversation.repository.as_deref(), Some("repo://mash"));
    }
    assert_eq!(service.get("freshness", FIRST).await, before);
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL with pgvector"]
async fn postgres_publication_excludes_own_node_before_candidate_limit_and_readiness() {
    exclude_own_contract(Service::new(true).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_publication_excludes_own_node_before_candidate_limit_and_readiness() {
    exclude_own_contract(Service::with_dimensions(false, true, 384).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_publication_configured_limits_and_chunked_canonical_data_round_trip() {
    let small = Service::with_limit(false, true, 384, 1024 * 1024).await;
    let large = Service::with_limit(false, true, 384, 32 * 1024 * 1024).await;
    let mut events = snapshot(1);
    let text = "hello ".repeat(450_000);
    events[2]["records"][0]["content"] = json!(text);
    events[2]["records"][0]["projection"] = json!(text);
    events[2]["records"][1]["content"] = json!(text);
    events[2]["records"][1]["projection"] = json!(text);
    let records = events[2]["records"].as_array().unwrap().clone();
    events.truncate(2);
    for record in records {
        events.push(json!({"type":"batch","kind":"messages","records":[record]}));
    }
    events.push(json!({"type":"finish"}));
    assert_eq!(
        small.upload(FIRST, &events).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(small.get("status", FIRST).await["messages"], 0);
    assert_eq!(large.upload(FIRST, &events).await.status(), StatusCode::OK);
    assert_eq!(large.get("status", FIRST).await["messages"], 3);
    let unchanged = vec![
        json!({"type":"start","instance":"snapshot-instance","revision":2,"base_revision":1,"snapshot":false,"forgets":[]}),
        json!({"type":"finish"}),
    ];
    assert_eq!(
        large.upload(FIRST, &unchanged).await.status(),
        StatusCode::OK
    );
    assert_eq!(large.get("status", FIRST).await["messages"], 3);
    let forget = large
        .request("forget", FIRST)
        .json(&json!({"instance":"snapshot-instance","revision":3,"ids":["conversation"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(forget.status(), StatusCode::OK);
    assert_eq!(large.get("status", FIRST).await["messages"], 0);
    small.cleanup().await;
    large.cleanup().await;
}
impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Service {
    async fn new(database: bool) -> Self {
        Self::new_backend(database, false).await
    }
    async fn new_backend(database: bool, redis: bool) -> Self {
        Self::with_dimensions(database, redis, 384).await
    }
    async fn with_dimensions(database: bool, redis: bool, dimensions: usize) -> Self {
        Self::with_limit(database, redis, dimensions, 64 * 1024 * 1024).await
    }
    async fn with_limit(
        database: bool,
        redis: bool,
        dimensions: usize,
        max_node_bytes: usize,
    ) -> Self {
        let generation = if dimensions == 384 {
            crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned()
        } else {
            crate::semantic::model_space("voyage-4", dimensions)
        };
        let directory = tempfile::tempdir().unwrap();
        let group = format!(
            "api-{}",
            directory.path().file_name().unwrap().to_string_lossy()
        );
        let url = if database {
            std::env::var("MASH_TEST_PG_URL").unwrap()
        } else {
            "host=127.0.0.1 port=9 user=unused connect_timeout=1".into()
        };
        let options = url.parse().unwrap();
        let pool = Pool::builder(Manager::new(options, tokio_postgres::NoTls))
            .max_size(4)
            .build()
            .unwrap();
        if database {
            backend::initialize(&pool, &[group.clone(), format!("{group}-other")])
                .await
                .unwrap();
        }
        let identities = Arc::new(HashMap::from([
            (
                blake3::hash(FIRST.as_bytes()),
                Identity {
                    group_id: group.clone(),
                    node_id: "first".into(),
                    generation: generation.clone(),
                    dimensions,
                },
            ),
            (
                blake3::hash(SECOND.as_bytes()),
                Identity {
                    group_id: group.clone(),
                    node_id: "second".into(),
                    generation: generation.clone(),
                    dimensions,
                },
            ),
            (
                blake3::hash(OTHER.as_bytes()),
                Identity {
                    group_id: format!("{group}-other"),
                    node_id: "first".into(),
                    generation: generation.clone(),
                    dimensions,
                },
            ),
        ]));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let store = if redis {
            Store::Redis(
                super::super::redis_index::Store::connect(
                    &std::env::var("MASH_TEST_REDIS_URL").unwrap(),
                    &[group.clone(), format!("{group}-other")],
                    dimensions,
                    max_node_bytes,
                )
                .await
                .unwrap(),
            )
        } else {
            Store::Postgres(pool.clone())
        };
        let app = App {
            store: store.clone(),
            identities,
            uploads: Arc::new(tokio::sync::Semaphore::new(4)),
        };
        let task = tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        });
        Self {
            pool,
            store,
            url,
            group,
            task,
        }
    }
    fn request(&self, route: &str, token: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .post(format!("{}/v1/{route}", self.url))
            .bearer_auth(token)
    }
    async fn get(&self, route: &str, token: &str) -> Value {
        reqwest::Client::new()
            .get(format!("{}/v1/{route}", self.url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn upload(&self, token: &str, events: &[Value]) -> reqwest::Response {
        use std::fmt::Write;
        let mut body = String::new();
        for event in events {
            writeln!(&mut body, "{event}").unwrap();
        }
        self.request("index", token)
            .header("content-type", "application/x-ndjson")
            .body(body)
            .send()
            .await
            .unwrap()
    }
    async fn cleanup(&self) {
        match &self.store {
            Store::Postgres(pool) => {
                let client = pool.get().await.unwrap();
                for group in [&self.group, &format!("{}-other", self.group)] {
                    client
                        .batch_execute(&format!(
                            "DROP SCHEMA {} CASCADE",
                            backend::group_schema(group)
                        ))
                        .await
                        .unwrap();
                }
            }
            Store::Redis(_) => {
                let client =
                    redis::Client::open(std::env::var("MASH_TEST_REDIS_URL").unwrap()).unwrap();
                let mut connection = client.get_multiplexed_async_connection().await.unwrap();
                for group in [&self.group, &format!("{}-other", self.group)] {
                    let prefix = format!("mash:r:{}:", blake3::hash(group.as_bytes()).to_hex());
                    redis::cmd("FT.DROPINDEX")
                        .arg(format!("{prefix}index"))
                        .arg("DD")
                        .query_async::<()>(&mut connection)
                        .await
                        .unwrap();
                    let keys: Vec<String> = redis::cmd("KEYS")
                        .arg(format!("{prefix}*"))
                        .query_async(&mut connection)
                        .await
                        .unwrap();
                    if !keys.is_empty() {
                        redis::cmd("DEL")
                            .arg(keys)
                            .query_async::<usize>(&mut connection)
                            .await
                            .unwrap();
                    }
                }
            }
        }
    }
}
fn snapshot(revision: i64) -> Vec<Value> {
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    let mut opposite = vector.clone();
    opposite[0] = -127.0;
    vec![
        json!({"type":"start","instance":"snapshot-instance","revision":revision,"base_revision":null,"snapshot":true,"forgets":[]}),
        json!({"type":"batch","kind":"conversations","records":[{"id":"conversation","provider":"codex","title":"Shared history","project":"/work/mash","repository":"repo://mash","created_at":1,"updated_at":2}]}),
        json!({"type":"batch","kind":"messages","records":[
            {"id":"searchable","conversation_id":"conversation","ordinal":0,"role":"user","content":"hello shared history","projection":"hello shared history","created_at":1,"generation":crate::semantic::SHARED_EMBEDDING_GENERATION,"vector":vector},
            {"id":"opposite","conversation_id":"conversation","ordinal":1,"role":"user","content":"opposite direction","projection":"opposite direction","created_at":2,"generation":crate::semantic::SHARED_EMBEDDING_GENERATION,"vector":opposite},
            {"id":"context","conversation_id":"conversation","ordinal":2,"role":"tool","content":"raw context","projection":"","created_at":2,"generation":null,"vector":null}
        ]}),
        json!({"type":"finish"}),
    ]
}

#[tokio::test]
async fn authorization_and_request_limits_apply_before_database_access() {
    let service = Service::new(false).await;
    let unauthorized = service
        .request("search", "wrong")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        unauthorized.json::<Value>().await.unwrap()["error"]["retryable"],
        false
    );
    let mismatch = service
        .request("search", FIRST)
        .header("x-mash-group", "wrong-group")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::FORBIDDEN);
    let invalid = service
        .request("search", FIRST)
        .json(&json!({"query":"hello","vector":[1],"provider":null,"days":null}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let malformed = service
        .request("view", FIRST)
        .json(&json!({"sql":"select secret"}))
        .send()
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!malformed.text().await.unwrap().contains("select secret"));
    let oversized = service
        .request("view", FIRST)
        .header("content-type", "application/json")
        .body("x".repeat(65537))
        .send()
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let line = service
        .request("index", FIRST)
        .header("content-type", "application/x-ndjson")
        .body("x".repeat(8 * 1024 * 1024 + 1))
        .send()
        .await
        .unwrap();
    assert_eq!(line.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL pointing to disposable PostgreSQL with pgvector"]
#[allow(clippy::too_many_lines)]
async fn postgres_publication_http_authorization_vectors_groups_rollback_and_retry() {
    http_storage_contract(Service::new(true).await).await;
}
#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL pointing to disposable Redis Search"]
async fn redis_publication_http_authorization_vectors_groups_rollback_and_retry() {
    http_storage_contract(Service::new_backend(false, true).await).await;
}
#[allow(clippy::too_many_lines)]
async fn http_storage_contract(service: Service) {
    let events = snapshot(1);
    let initial = service.upload(FIRST, &events).await;
    assert_eq!(
        initial.status(),
        StatusCode::OK,
        "{}",
        initial.text().await.unwrap()
    );
    let freshness = service.get("freshness", FIRST).await;
    let duplicate = service.upload(FIRST, &events).await;
    assert_eq!(duplicate.status(), StatusCode::OK);
    let retry = service.get("freshness", FIRST).await;
    assert_eq!(
        retry["nodes"][0]["last_updated_at"],
        freshness["nodes"][0]["last_updated_at"]
    );
    assert!(
        retry["nodes"][0]["last_synced_at"].as_str()
            > freshness["nodes"][0]["last_synced_at"].as_str()
    );
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    let search = json!({"query":"hello","vector":vector,"provider":null,"days":null});
    let candidates: Value = service
        .request("search", FIRST)
        .json(&search)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(candidates["lexical"].as_array().unwrap().len(), 1);
    assert_eq!(candidates["semantic"][0]["id"], "searchable");
    assert_eq!(candidates["semantic"][0]["semantic_score"], 1.0);
    assert_eq!(candidates["semantic"][1]["semantic_score"], -1.0);
    for kind in ["lexical", "semantic"] {
        assert_eq!(candidates[kind][0]["metadata"]["title"], "Shared history");
        assert_eq!(candidates[kind][0]["metadata"]["project"], "/work/mash");
        assert_eq!(candidates[kind][0]["metadata"]["repository"], "repo://mash");
        assert_eq!(candidates[kind][0]["metadata"]["updated_at"], 2);
        assert_eq!(candidates[kind][0]["created_at"], 1);
    }
    assert_eq!(
        service.upload(SECOND, &events).await.status(),
        StatusCode::OK
    );
    let candidates: Value = service
        .request("search", SECOND)
        .json(&search)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(candidates["lexical"].as_array().unwrap().len(), 2);
    let view = json!({"node_id":"first","id":"searchable","context":2});
    let context: Vec<Value> = service
        .request("view", SECOND)
        .json(&view)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(context.len(), 3);
    let projected: Vec<Value> = service
        .request("view", SECOND)
        .json(
            &json!({"node_id":"first","id":"searchable","context":2,"content":"search-projection"}),
        )
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(projected[0]["content"], "hello shared history");
    assert_eq!(projected[2]["content"], "");
    assert_eq!(context[2]["content"], "raw context");
    let other: Vec<Value> = service
        .request("view", OTHER)
        .json(&view)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(other, Vec::<Value>::new());
    let fresh_before = service.get("freshness", FIRST).await;
    let mut invalid = snapshot(2);
    invalid[2]["records"][0]["vector"] = json!([1, 2]);
    assert_eq!(
        service.upload(FIRST, &invalid).await.status(),
        StatusCode::BAD_REQUEST
    );
    let mut truncated = snapshot(2);
    truncated.pop();
    assert_eq!(
        service.upload(FIRST, &truncated).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(service.get("freshness", FIRST).await, fresh_before);
    assert_eq!(service.get("status", FIRST).await["messages"], 3);
    let mut changed_instance = snapshot(2);
    changed_instance[0]["instance"] = json!("other-local-db");
    assert_eq!(
        service.upload(FIRST, &changed_instance).await.status(),
        StatusCode::CONFLICT
    );
    let current = snapshot(2);
    let (left, right) = tokio::join!(
        service.upload(FIRST, &current),
        service.upload(FIRST, &current)
    );
    assert!(left.status() == StatusCode::OK || right.status() == StatusCode::OK);
    assert!(matches!(
        left.status(),
        StatusCode::OK | StatusCode::CONFLICT
    ));
    assert!(
        matches!(right.status(), StatusCode::OK | StatusCode::CONFLICT),
        "{}",
        right.text().await.unwrap()
    );
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::CONFLICT
    );
    let deleted = service
        .request("forget", FIRST)
        .json(&json!({"instance":"snapshot-instance","revision":3,"ids":["conversation"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(service.get("status", FIRST).await["messages"], 0);
    assert_eq!(service.get("status", SECOND).await["messages"], 3);
    assert_eq!(
        service.upload(FIRST, &snapshot(4)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        service.get("status", FIRST).await["messages"],
        0,
        "tombstones survive replay"
    );
    let mut old_generation = snapshot(2);
    old_generation[2]["records"][0]["generation"] = json!("older-model-space");
    assert_eq!(
        service.upload(SECOND, &old_generation).await.status(),
        StatusCode::OK
    );
    let incompatible = service
        .request("search", FIRST)
        .json(&search)
        .send()
        .await
        .unwrap();
    assert_eq!(incompatible.status(), StatusCode::SERVICE_UNAVAILABLE);
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL pointing to disposable PostgreSQL with pgvector"]
async fn postgres_publication_migrates_existing_array_vectors_without_losing_history() {
    let service = Service::new(true).await;
    let client = service.pool.get().await.unwrap();
    let schema = backend::group_schema(&service.group);
    client.batch_execute(&format!("SET search_path={schema},public; INSERT INTO nodes VALUES('legacy','old',1); INSERT INTO conversations VALUES('legacy','conversation','codex',NULL,NULL,NULL); ALTER TABLE messages ALTER COLUMN vector TYPE double precision[] USING NULL; UPDATE version SET version=1;")).await.unwrap();
    for (id, vector) in [
        ("complete", vec![1.0f64; 384]),
        ("incomplete", vec![1.0; 2]),
    ] {
        client.execute("INSERT INTO messages(node_id,id,conversation_id,ordinal,role,content,projection,generation,vector) VALUES('legacy',$1,'conversation',$2,'user','legacy history','legacy history',$3,$4)", &[&id,&(i64::from(id!="complete")),&crate::semantic::SHARED_EMBEDDING_GENERATION,&vector]).await.unwrap();
    }
    drop(client);
    backend::initialize(&service.pool, std::slice::from_ref(&service.group))
        .await
        .unwrap();
    let client = service.pool.get().await.unwrap();
    let tx=client.query_one(&format!("SELECT count(*),count(vector),max(public.vector_dims(vector)) FROM {schema}.messages"),&[]).await.unwrap();
    assert_eq!(tx.get::<_, i64>(0), 2);
    assert_eq!(tx.get::<_, i64>(1), 1);
    assert_eq!(tx.get::<_, Option<i32>>(2), Some(384));
    drop(client);
    service.cleanup().await;
}
