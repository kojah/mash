use super::*;

#[tokio::test]
async fn superseded_mcp_checks_admission_after_the_mutation_gate() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("mash");
    std::fs::write(&executable, "running").unwrap();
    let database = root.path().join("index.sqlite3");
    let paths = Paths {
        db: Some(database.clone()),
        ..Paths::default()
    };
    let lease = crate::runtime::Lease::fixture(&paths, &executable);
    let server = Mash {
        runtime: Some(Arc::new(lease)),
        paths: Arc::new(paths),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let guard = server.gate.lock().await;
    let operation = server.perform_response(Operation::Index(core::IndexRequest::default()));
    tokio::pin!(operation);
    // Poll the actual admission boundary while another mutation owns the gate.
    assert!(futures_util::poll!(&mut operation).is_pending());
    std::fs::write(&executable, "replaced executable").unwrap();
    drop(guard);
    let error = operation.await.unwrap_err();
    assert!(error.error.message.contains("executable was replaced"));
    assert!(!database.exists());
    let help = server
        .help(Parameters(crate::mcp::guidance::Help {
            tool: None,
            topic: None,
            expected_contract: None,
        }))
        .await;
    assert_ne!(help.is_error, Some(true));
}
