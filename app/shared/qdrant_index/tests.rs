use super::*;
use crate::shared::protocol::{self, Batch, ConversationRecord, Event, MessageRecord, Start};
use crate::shared::server::store::Store as Server;
use crate::shared::silver_store::SilverStore;
use std::io::Write;
mod exact;
mod memory;
mod physical;
mod recipe;
mod recovery;

/// One disposable group: a SQLite silver file and its Qdrant gold collections.
struct Fixture {
    store: Store,
    server: Server,
    silver: SilverStore,
    _directory: tempfile::TempDir,
    identity: Identity,
    url: String,
}
impl Fixture {
    async fn new(index: VectorIndex) -> Self {
        Self::with_memory(index, MemoryTier::Cached).await
    }

    async fn with_memory(index: VectorIndex, memory: MemoryTier) -> Self {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).unwrap();
        let group = format!("qdrant-test-{}", hex::encode(random));
        let url = std::env::var("MASH_TEST_QDRANT_URL").unwrap();
        let identity = Identity {
            group_id: group.clone(),
            node_id: "node".into(),
            generation: crate::semantic::model_space("test", 4),
            dimensions: 4,
        };
        let store = Store::connect(
            &url,
            None,
            &[group],
            4,
            64 * 1024 * 1024,
            index,
            memory.into(),
        )
        .await
        .unwrap();
        store.select_gold(&identity, false).await.unwrap();
        let directory = tempfile::tempdir().unwrap();
        let silver = SilverStore::open(&directory.path().join("silver"), [&identity]).unwrap();
        Self {
            server: Server::new(silver.clone(), store.clone(), 64 * 1024 * 1024),
            store,
            silver,
            _directory: directory,
            identity,
            url,
        }
    }

    /// A service process over the same silver with a separately configured gold handle.
    fn serve(&self, gold: Store) -> Server {
        Server::new(self.silver.clone(), gold, 64 * 1024 * 1024)
    }

    async fn reopen(&self, index: VectorIndex, memory: MemoryTier) -> Result<Server, AppError> {
        let gold = Store::connect(
            &self.url,
            None,
            std::slice::from_ref(&self.identity.group_id),
            self.identity.dimensions,
            64 * 1024 * 1024,
            index,
            memory.into(),
        )
        .await?;
        gold.select_gold(&self.identity, false).await?;
        Ok(self.serve(gold))
    }

    /// The node's retained silver, serialized, for before/after comparison.
    fn retained(&self) -> Vec<u8> {
        let connection = self.silver.connect(&self.identity).unwrap();
        let mut output = Vec::new();
        crate::shared::silver_store::read::export(&connection, &mut output).unwrap();
        output
    }

    async fn cleanup(&self) {
        for meta in [false, true] {
            reqwest::Client::new()
                .delete(format!(
                    "{}/collections/{}",
                    self.url,
                    collection(&self.identity.group_id, meta)
                ))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        }
    }

    fn upload(events: &[Event]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for event in events {
            writeln!(file, "{}", serde_json::to_string(event).unwrap()).unwrap();
        }
        file
    }

    fn conversation() -> ConversationRecord {
        ConversationRecord {
            enrichment: None,
            id: "thread".into(),
            provider: crate::provider::Provider::Codex,
            title: Some("Needle".into()),
            project: Some("project".into()),
            repository: Some("repo".into()),
            created_at: Some(1000),
            updated_at: Some(2000),
        }
    }

    fn messages(&self, marker: &str) -> Vec<MessageRecord> {
        (0..2)
            .map(|i| MessageRecord {
                attachments: Vec::new(),
                id: format!("message-{i}"),
                conversation_id: "thread".into(),
                ordinal: i,
                context: crate::message_context::MessageContext::Linear,
                role: crate::role::Role::User,
                content: format!("needle canonical {marker}"),
                projection: format!("needle {marker}"),
                created_at: Some(1000 + i * 1000),
                generation: Some(self.identity.generation.clone()),
                vector: Some(vec![127.0, 0.0, 0.0, 0.0]),
            })
            .collect()
    }

    fn start(revision: i64) -> Start {
        Start {
            reuse_gold: false,
            expected_version: None,
            gold_build: None,
            source_silver: None,
            instance: "instance".into(),
            revision: Some(revision),
            base_revision: None,
            snapshot: true,
            forgets: vec![],
        }
    }

    fn file(&self, revision: i64, marker: &str) -> tempfile::NamedTempFile {
        Self::upload(&[
            Event::Start(Self::start(revision)),
            Event::Batch(Batch::Conversations {
                records: vec![Self::conversation()],
            }),
            Event::Batch(Batch::Messages {
                records: self.messages(marker),
            }),
            Event::Finish,
        ])
    }

    /// A delta publication that only forgets threads.
    fn forget(revision: i64, ids: &[&str]) -> tempfile::NamedTempFile {
        let mut start = Self::start(revision);
        start.snapshot = false;
        start.base_revision = Some(revision - 1);
        start.forgets = ids.iter().map(|id| (*id).to_owned()).collect();
        Self::upload(&[Event::Start(start), Event::Finish])
    }

    fn query(&self) -> protocol::Search {
        let mut vector = vec![0.0; self.identity.dimensions];
        vector[0] = 127.0;
        protocol::Search {
            intent: crate::gold::exact::Intent::RelatedDiscussion,
            vector_policy: None,
            unit: crate::search_results::SearchUnit::Messages,
            query: "needle".into(),
            generation: self.identity.generation.clone(),
            vector,
            provider: None,
            days: None,
            exclude_self: false,
            filter: None,
        }
    }

    async fn search(&self) -> protocol::Candidates {
        self.server
            .search(&self.identity, self.query())
            .await
            .unwrap()
    }
}

#[test]
fn qdrant_configuration_and_sparse_vectors_are_typed_and_bounded() {
    let sparse = sparse("needle needle other", false);
    assert_eq!(sparse.indices.len(), 2);
    assert_eq!(sparse.indices.len(), sparse.values.len());
    assert!(sparse.values.iter().all(|v| v.is_finite() && *v > 0.0));
    assert_eq!(
        document_id("node", "message"),
        document_id("node", "message")
    );
    assert_ne!(
        document_id("node", "message"),
        document_id("other", "message")
    );
}
