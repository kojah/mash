use super::*;
use serde_json::{Value, json};
use std::collections::HashSet;

mod admission;
mod antigravity;
mod attachments;
mod chats;
mod connectors;
mod context;
mod contexts;
mod correlation;
mod cursor;
mod enrichment;
mod exact;
mod freshness;
mod gold;
mod native;
mod redaction;
mod relevance;
mod restart;
mod selections;
mod silver_revision;
mod source_silver;

const FIRST: &str = "test-api-token-first-32-bytes-long";
const SECOND: &str = "test-api-token-second-32-bytes-long";
const OTHER: &str = "test-api-token-other-32-bytes-long";

fn provider_snapshot(vector: &[f64], revision: i64) -> Vec<Value> {
    use crate::provider::Provider;
    use crate::shared::protocol::{Batch, ConversationRecord, Event, MessageRecord, Start};
    let conversations = Provider::ALL
        .iter()
        .map(|provider| ConversationRecord {
            enrichment: None,
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
            attachments: Vec::new(),
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
            gold_build: None,
            source_silver: None,
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
                match_mode: crate::gold::matching::Mode::Strict,
                intent: crate::gold::exact::Intent::RelatedDiscussion,
                vector_policy: None,
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
            assert_eq!(
                result.lexical[0].source,
                crate::source::Source::Provider(provider)
            );
            assert_eq!(
                result.semantic[0].source,
                crate::source::Source::Provider(provider)
            );
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
    service.cleanup().await;
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
    assert_eq!(service.counts(FIRST).await.embeddings, 2);
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

struct Service {
    _attachments: tempfile::TempDir,
    silver_dir: tempfile::TempDir,
    url: String,
    group: String,
    task: tokio::task::JoinHandle<()>,
    uploads: Arc<tokio::sync::Semaphore>,
    store: Store,
    first: Identity,
    identities: Arc<HashMap<blake3::Hash, Identity>>,
}

#[derive(Clone, Copy)]
enum TestBackend {
    Unavailable,
    Silver,
    Qdrant,
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
    let filter = json!({"all":[{"node":["first"]},{"provider":["codex"]},{"role":["assistant"]},{"chat":["conversation"]},{"project":["/work/alpha"]},{"repository":[repository]},{"after":2},{"before":3}]});
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

impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to disposable Qdrant 1.19+"]
async fn qdrant_publication_http_authorization_vectors_groups_rollback_and_retry() {
    http_storage_contract(Service::qdrant(384).await).await;
}
#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to disposable Qdrant 1.19+"]
async fn qdrant_publication_rich_filters_apply_before_ranking() {
    rich_filter_contract(Service::qdrant(384).await).await;
}
#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to disposable Qdrant 1.19+"]
async fn qdrant_publication_excludes_own_node_before_candidate_limit_and_readiness() {
    exclude_own_contract(Service::qdrant(384).await).await;
}
#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to disposable Qdrant 1.19+"]
async fn qdrant_round_trips_every_supported_provider_with_filters() {
    provider_contract(Service::qdrant(384).await).await;
}
#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to disposable Qdrant 1.19+"]
async fn qdrant_publication_remote_model_space_supports_1024_dimensions() {
    remote_space_contract(Service::qdrant(1024).await).await;
}
impl Service {
    async fn new(available: bool) -> Self {
        Self::with_parameters(available, 384, 64 * 1024 * 1024, FIRST).await
    }
    async fn qdrant(dimensions: usize) -> Self {
        Self::with_parameters(true, dimensions, 64 * 1024 * 1024, FIRST).await
    }
    async fn with_parameters(
        available: bool,
        dimensions: usize,
        max_node_bytes: usize,
        first_token: &str,
    ) -> Self {
        Self::with_backend(
            if available {
                TestBackend::Qdrant
            } else {
                TestBackend::Unavailable
            },
            dimensions,
            max_node_bytes,
            first_token,
        )
        .await
    }

    async fn canonical() -> Self {
        Self::with_backend(TestBackend::Silver, 384, 64 * 1024 * 1024, FIRST).await
    }

    async fn with_backend(
        backend: TestBackend,
        dimensions: usize,
        max_node_bytes: usize,
        first_token: &str,
    ) -> Self {
        let available = matches!(backend, TestBackend::Qdrant);
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
        let gold = if available {
            super::super::qdrant_index::Store::connect(
                &std::env::var("MASH_TEST_QDRANT_URL").unwrap(),
                None,
                &[group.clone(), format!("{group}-other")],
                dimensions,
                64 * 1024 * 1024,
                super::super::qdrant_index::VectorIndex::default(),
                super::super::qdrant_index::MemoryPolicy::default(),
            )
            .await
            .unwrap()
        } else {
            super::super::qdrant_index::Store::unavailable()
        };
        let identities = Arc::new(HashMap::from([
            (
                blake3::hash(first_token.as_bytes()),
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
                    generation,
                    dimensions,
                },
            ),
        ]));
        if available {
            for identity in identities.values() {
                gold.select_gold(identity, false).await.unwrap();
            }
        }
        let store = if matches!(backend, TestBackend::Unavailable) {
            Store::unavailable()
        } else {
            let silver = super::super::silver_store::SilverStore::open(
                &directory.path().join("silver"),
                identities.values(),
            )
            .unwrap();
            Store::new(silver, gold, max_node_bytes as u64)
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let uploads = Arc::new(tokio::sync::Semaphore::new(4));
        let attachments = tempfile::tempdir().unwrap();
        let first = identities[&blake3::hash(first_token.as_bytes())].clone();
        let app = App {
            attachments_dir: Some(attachments.path().to_path_buf()),
            store: store.clone(),
            identities: identities.clone(),
            uploads: uploads.clone(),
        };
        let task = tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        });
        Self {
            _attachments: attachments,
            silver_dir: directory,
            url,
            group,
            task,
            uploads,
            store,
            first,
            identities,
        }
    }
    /// One node's silver and gold counts, read from the store directly.
    async fn counts(&self, token: &str) -> crate::storage::Counts {
        let identity = &self.identities[&blake3::hash(token.as_bytes())];
        self.store.status(identity).await.unwrap()
    }
    /// The first node's publication state, read from the store directly.
    async fn state(&self) -> protocol::NodeState {
        self.store.state(&self.first).await.unwrap()
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
        for group in [&self.group, &format!("{}-other", self.group)] {
            for suffix in ["state", "messages"] {
                reqwest::Client::new()
                    .delete(format!(
                        "{}/collections/mash_gold_{}_{suffix}",
                        std::env::var("MASH_TEST_QDRANT_URL").unwrap(),
                        blake3::hash(group.as_bytes()).to_hex()
                    ))
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap();
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
    assert_eq!(service.counts(FIRST).await.messages, 3);
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
    // Forgets travel in the next publication's start event.
    let deleted = service
        .upload(
            FIRST,
            &[
                json!({"type":"start","instance":"snapshot-instance","revision":3,"base_revision":2,"snapshot":false,"forgets":["conversation"]}),
                json!({"type":"finish"}),
            ],
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(service.counts(FIRST).await.messages, 0);
    assert_eq!(service.counts(SECOND).await.messages, 3);
    assert_eq!(
        service.upload(FIRST, &snapshot(4)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        service.counts(FIRST).await.messages,
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
