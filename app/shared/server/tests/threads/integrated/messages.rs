use super::*;

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL and installed MASH_TEST_MODELS_DIR"]
async fn qdrant_core_scoped_message_search_uses_thread_discovery() {
    let token = std::env::var("PATH").unwrap();
    contract(
        Service::with_backend(false, TestBackend::Qdrant, 384, 64 * 1024 * 1024, &token).await,
    )
    .await;
}
use crate::search_filter::Selection;
use crate::search_results::{Results, SearchUnit};
use crate::semantic::{self, Models};
use crate::silver::{Conversation, NormalizedMessage, SourceIdentity};

fn conversation(id: &str, provider: Provider) -> Conversation {
    Conversation {
        id: id.into(),
        provider,
        claude_lineage: None,
        source: SourceIdentity::file(id),
        title: Some(format!("Authentication {id}")),
        project: None,
        repository: None,
        created_at: Some(1000),
        updated_at: Some(2000),
        messages: vec![NormalizedMessage {
            id: format!("{id}-message"),
            ordinal: 0,
            context: crate::message_context::MessageContext::Linear,
            role: crate::role::Role::User,
            content: "repair authentication credentials".into(),
            dialogue_text: None,
            created_at: Some(2000),
        }]
        .into(),
    }
}

async fn seed(paths: &Paths) {
    let database = paths.db.clone().unwrap();
    let models = paths.models_dir.clone().unwrap();
    crate::blocking(move || {
        let mut storage = Storage::open_writer(&database)?;
        for number in 0..150 {
            storage.replace_conversation(&conversation(
                &format!("unselected-{number:03}"),
                Provider::Codex,
            ))?;
        }
        for provider in [Provider::Hermes, Provider::Codex, Provider::ClaudeCode] {
            storage.replace_conversation(&conversation(provider.as_str(), provider))?;
        }
        storage.rebuild_derived_search_state()?;
        let models = Models::new(models);
        let mut pool = models.load_indexer()?.unwrap();
        semantic::rebuild_embeddings(&mut storage, &mut pool, &models.generation(), |_| {})?;
        storage.mark_semantic_index_ready(&models.generation())?;
        storage.commit_writer()
    })
    .await
    .unwrap();
}

async fn assert_scoped(paths: &Paths, ids: &[String], expected: &[&str]) {
    let response = core::execute(core::Request {
        paths: paths.clone(),
        operation: Operation::Search {
            query: "authentication".into(),
            limit: None,
            unit: None,
            filters: Box::new(Selection {
                provider: vec![Provider::Codex, Provider::ClaudeCode],
                conversation: ids.to_vec(),
                after: Some(2),
                before: Some(3),
                ..Default::default()
            }),
            local: false,
            shared: false,
        },
    })
    .await
    .unwrap();
    let core::Response::Search(response) = response else {
        panic!("search response")
    };
    assert!(response.shared, "{:?}", response.shared_error);
    assert_eq!(response.unit, SearchUnit::Messages);
    let Results::Messages(messages) = response.results else {
        panic!("messages")
    };
    let mut found: Vec<_> = messages
        .iter()
        .map(|message| message.conversation_id.as_str())
        .collect();
    found.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(found, expected);
    assert!(
        messages.iter().all(|message| {
            let nodes: std::collections::HashSet<_> = std::iter::once(message.node_id.as_deref())
                .chain(message.copies.iter().map(|copy| copy.node_id.as_deref()))
                .collect();
            message.copies.len() == 1
                && nodes == std::collections::HashSet::from([None, Some("second")])
        }),
        "local and peer copies should merge into one hit: {messages:?}"
    );
}

async fn contract(service: Service) {
    let directory = tempfile::tempdir().unwrap();
    let mut paths = configuration(directory.path(), &service);
    let config_path = paths.config.as_ref().unwrap();
    let configuration = std::fs::read_to_string(config_path).unwrap();
    std::fs::write(
        config_path,
        format!("{configuration}\n[gold.query]\nunit = 'messages'\nlimit = 10\n"),
    )
    .unwrap();
    paths.models_dir = Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into());
    seed(&paths).await;
    let config = crate::config::load(paths.config.as_deref())
        .unwrap()
        .shared
        .unwrap();
    assert!(
        !crate::shared::publish(paths.db.as_deref().unwrap(), &config)
            .await
            .is_pending()
    );
    // The backend fixture permits explicit peer tokens without changing process environment.
    let mut snapshot = provider_snapshot(&vec![127.0; 384], 1);
    for row in snapshot[2]["records"].as_array_mut().unwrap() {
        row["content"] = json!("repair authentication credentials");
        row["projection"] = json!("repair authentication credentials");
        row["created_at"] = json!(2000);
    }
    service
        .upload(SECOND, &snapshot)
        .await
        .error_for_status()
        .unwrap();
    let discovered = discover(
        &paths,
        Request {
            provider: vec![Provider::Codex, Provider::ClaudeCode],
            title_contains: Some("Authentication".into()),
            after: Some(2),
            before: Some(3),
            limit: 100,
            ..Default::default()
        },
        false,
    )
    .await;
    let ids: Vec<_> = discovered
        .threads
        .iter()
        .filter(|thread| ["codex", "claude-code"].contains(&thread.conversation_id.as_str()))
        .map(|thread| thread.conversation_id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_scoped(&paths, &ids, &["codex", "claude-code"]).await;
    assert_scoped(&paths, &["unknown".into()], &[]).await;
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL and MASH_TEST_MODELS_DIR"]
async fn postgres_real_models_discovery_to_merged_scoped_messages() {
    let token = std::env::var("PATH").unwrap();
    contract(Service::with_token(true, false, 384, 64 * 1024 * 1024, &token).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL and MASH_TEST_MODELS_DIR"]
async fn redis_real_models_discovery_to_merged_scoped_messages() {
    let token = std::env::var("PATH").unwrap();
    contract(Service::with_token(false, true, 384, 64 * 1024 * 1024, &token).await).await;
}
