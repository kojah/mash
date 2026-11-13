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

#[tokio::test]
async fn record_waits_boundedly_for_the_writer_and_rechecks_the_runtime_after() {
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
    let mutation = || -> crate::records::Mutation {
        serde_json::from_value(json!({"operation_id":"wait","change":{"action":"create","scope":{"kind":"chat","id":"c"},"body":{"statement":"Use Qdrant","review":"candidate","recorded_by":"test agent","details":{"kind":"decision","state":"proposed"}},"evidence":[{"message_id":"m","quote":"q","purpose":"context"}]}}))
        .unwrap()
    };
    let never = std::future::pending::<()>;
    let pass = crate::storage::admission::acquire(&database).unwrap();
    // A pass outlasting the bound yields a retryable error asking for a retry.
    let error = server
        .record_within(mutation(), std::time::Duration::from_millis(250), never())
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
    assert!(error.error.retryable);
    assert!(error.error.message.contains("retry this call"));
    // So does an in-process mutation holding the MCP gate.
    let gate = server.gate.lock().await;
    let error = server
        .record_within(mutation(), std::time::Duration::from_millis(250), never())
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
    drop(gate);
    // The executable is replaced while record waits for the pass: once
    // admitted, the superseded runtime refuses the write.
    let waiting = server.record_within(mutation(), std::time::Duration::from_secs(60), never());
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), &mut waiting)
            .await
            .is_err()
    );
    std::fs::write(&executable, "replaced executable").unwrap();
    drop(pass);
    let error = tokio::time::timeout(crate::test_support::HANG_GUARD, waiting)
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.error.message.contains("executable was replaced"));
    assert!(!database.exists());
}
