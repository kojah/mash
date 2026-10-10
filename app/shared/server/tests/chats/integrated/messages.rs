use super::*;

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL and installed MASH_TEST_MODELS_DIR"]
async fn qdrant_core_scoped_message_search_uses_thread_discovery() {
    let token = std::env::var("PATH").unwrap();
    contract(Service::with_parameters(true, 384, 64 * 1024 * 1024, &token).await).await;
}
use crate::search_filter::Selection;
use crate::search_results::{Results, SearchUnit};
use crate::semantic::{self, Models};
use crate::silver::{Conversation, NormalizedMessage, SourceIdentity};

fn conversation(id: &str, provider: Provider) -> Conversation {
    Conversation {
        id: id.into(),
        provider,
        title_origin: None,
        timestamp_origins: None,
        claude_lineage: None,
        source: SourceIdentity::file(id),
        title: Some(format!("Authentication {id}")),
        project: None,
        repository: None,
        created_at: Some(1000),
        updated_at: Some(2000),
        messages: vec![NormalizedMessage {
            attachments: Vec::new(),
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
        for provider in [Provider::Opencode, Provider::Codex, Provider::ClaudeCode] {
            storage.replace_conversation(&conversation(provider.as_str(), provider))?;
        }
        storage.rebuild_derived_search_state()?;
        let models = Models::new(models);
        let mut pool = models.load_indexer()?.unwrap();
        semantic::rebuild_embeddings(&mut storage, &mut pool, &models.generation(), |_| {})?;
        storage.mark_semantic_index_ready(&models.generation())?;
        for provider in [Provider::Opencode, Provider::Codex, Provider::ClaudeCode] {
            storage.begin_silver_scan(provider)?;
            storage.finish_silver_scan(provider, true)?;
        }
        storage.finish_silver_ingestion()?;
        storage.mark_semantic_index_ready(&models.generation())?;
        storage.publish_lexical_manifest()?;
        storage.publish_views_manifest(&crate::views::manifest::Manifest::new(
            crate::views::config::Build::default(),
            models.generation(),
            storage.silver_certificate()?,
        ))?;
        storage.commit_writer()
    })
    .await
    .unwrap();
}

async fn assert_scoped(
    paths: &Paths,
    ids: &[String],
    expected: &[&str],
    ranking: Option<crate::views::config::Ranking>,
    vector_policy: Option<crate::views::vector::Search>,
) {
    let response = core::execute(core::Request {
        paths: paths.clone(),
        operation: Operation::Search {
            shared_vector_search: vector_policy,
            fusion: crate::views::config::FusionOverrides::default(),
            ranking,
            diagnostics: true,
            query: "authentication".into(),
            intent: None,
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
    let ranking = ranking.unwrap_or(
        crate::config::load(paths.config.as_deref())
            .unwrap()
            .views
            .query
            .ranking,
    );
    let diagnostics = response.diagnostics.as_ref().unwrap();
    assert_eq!(diagnostics.shared_vector_override, vector_policy);
    assert_eq!(diagnostics.ranking, Some(ranking));
    if ranking == crate::views::config::Ranking::Fusion {
        assert!(diagnostics.stages.iter().all(|stage| !matches!(
            stage.stage,
            crate::observability::retrieval::Stage::Rerank
                | crate::observability::retrieval::Stage::Context
        )));
    }
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
        format!("{configuration}\n[views.query]\nunit = 'messages'\nlimit = 10\n"),
    )
    .unwrap();
    paths.models_dir = Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into());
    seed(&paths).await;
    let config = crate::config::load(paths.config.as_deref())
        .unwrap()
        .shared
        .unwrap();
    assert!(
        !crate::shared::publish(paths.db.as_deref().unwrap(), &config, None)
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
        .chats
        .iter()
        .filter(|thread| ["codex", "claude-code"].contains(&thread.conversation_id.as_str()))
        .map(|thread| thread.conversation_id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    let before = std::fs::read(paths.db.as_ref().unwrap()).unwrap();
    let token = std::env::var("PATH").unwrap();
    let shared_before = service.get("freshness", &token).await;
    for (ranking, override_ranking) in [
        (
            "contextual-rerank",
            Some(crate::views::config::Ranking::Fusion),
        ),
        ("contextual-rerank", None),
        (
            "fusion",
            Some(crate::views::config::Ranking::ContextualRerank),
        ),
        ("fusion", None),
    ] {
        std::fs::write(
            config_path,
            format!(
                "{configuration}\n[views.query]\nunit='messages'\nlimit=10\nranking='{ranking}'\n"
            ),
        )
        .unwrap();
        assert_scoped(
            &paths,
            &ids,
            &["codex", "claude-code"],
            override_ranking,
            None,
        )
        .await;
        assert_eq!(std::fs::read(paths.db.as_ref().unwrap()).unwrap(), before);
        assert_eq!(service.get("freshness", &token).await, shared_before);
    }
    for vector_policy in [
        Some(crate::views::vector::Search::Hnsw { ef_runtime: 80 }),
        Some(crate::views::vector::Search::Exact {}),
        None,
    ] {
        assert_scoped(&paths, &ids, &["codex", "claude-code"], None, vector_policy).await;
        assert_eq!(std::fs::read(paths.db.as_ref().unwrap()).unwrap(), before);
        assert_eq!(service.get("freshness", &token).await, shared_before);
    }
    assert_scoped(&paths, &["unknown".into()], &[], None, None).await;
    service.cleanup().await;
}
