use super::*;

fn mutation(trace_id: Option<TraceId>) -> WorkerRequest {
    WorkerRequest {
        request: Request {
            paths: crate::core::Paths {
                config: None,
                db: None,
                models_dir: None,
            },
            operation: crate::core::Operation::Forget {
                id: "private-history-canary".into(),
            },
        },
        trace_id,
    }
}

#[tokio::test]
async fn worker_diagnostics_correlate_rejected_mutations_without_payloads() {
    use tracing::instrument::WithSubscriber;
    let (bytes, dispatcher) = crate::observability::test_support::capture();
    let trace_id = TraceId::generate().unwrap();
    let wire = serde_json::to_vec(&mutation(Some(trace_id.clone()))).unwrap();
    let reply = execute_request(&wire)
        .with_subscriber(dispatcher.clone())
        .await;
    assert_eq!(reply.code, 2);
    assert_eq!(
        reply.result.as_ref().unwrap_err().error.kind,
        crate::ErrorKind::Usage
    );
    assert_eq!(
        reply.diagnostics.as_ref().unwrap().process_id,
        std::process::id()
    );
    tracing::dispatcher::with_default(&dispatcher, || {
        assert!(!report_diagnostics(
            &reply,
            Some(&TraceId::generate().unwrap())
        ));
        assert!(!report_diagnostics(&reply, None));
        assert!(report_diagnostics(&reply, Some(&trace_id)));
    });
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(output.contains(trace_id.as_str()));
    assert!(output.contains("worker_elapsed_ms"));
    assert!(output.contains("\"outcome\":\"error\""));
    assert!(!output.contains("private-history-canary"));
    assert_eq!(output.matches("worker reply").count(), 1);
}

#[test]
fn malformed_stage_summaries_are_rejected_before_logging() {
    let trace_id = TraceId::generate().unwrap();
    let timing = stages::Timing {
        stage: stages::Stage::Embed,
        calls: 1,
        total_us: 7,
        max_us: 7,
    };
    let mut reply = Reply {
        code: 2,
        result: Err(AppError::usage("invalid search worker request")),
        diagnostics: Some(Diagnostics {
            trace_id: trace_id.clone(),
            process_id: 1,
            elapsed_ms: 1,
            stages: vec![timing.clone()],
        }),
    };
    assert!(report_diagnostics(&reply, Some(&trace_id)));
    let diagnostics = reply.diagnostics.as_mut().unwrap();
    diagnostics.stages.push(timing);
    assert!(!report_diagnostics(&reply, Some(&trace_id)));
    let diagnostics = reply.diagnostics.as_mut().unwrap();
    diagnostics.stages.truncate(1);
    diagnostics.stages[0].calls = 0;
    assert!(!report_diagnostics(&reply, Some(&trace_id)));
    let diagnostics = reply.diagnostics.as_mut().unwrap();
    diagnostics.stages[0].calls = 1;
    diagnostics.stages[0].max_us = 8;
    assert!(!report_diagnostics(&reply, Some(&trace_id)));
}

#[tokio::test]
async fn worker_wire_rejects_unvalidated_trace_ids_and_omits_disabled_diagnostics() {
    let wire = serde_json::to_vec(&mutation(None)).unwrap();
    assert!(
        !String::from_utf8(wire.clone())
            .unwrap()
            .contains("trace_id")
    );
    let reply = execute_request(&wire).await;
    assert!(reply.diagnostics.is_none());
    assert!(report_diagnostics(&reply, None));
    assert!(
        !serde_json::to_string(&reply)
            .unwrap()
            .contains("diagnostics")
    );
    for invalid in [
        "private-token-canary",
        "0123456789ABCDEF0123456789ABCDEF",
        "abc",
    ] {
        let wire = format!(
            "{{\"request\":{},\"trace_id\":{}}}",
            serde_json::to_string(&mutation(None).request).unwrap(),
            serde_json::to_string(invalid).unwrap()
        );
        let reply = execute_request(wire.as_bytes()).await;
        assert!(reply.diagnostics.is_none());
        assert_eq!(reply.code, 2);
        assert!(!serde_json::to_string(&reply).unwrap().contains(invalid));
    }
}

#[tokio::test]
async fn invalid_searches_return_usage_without_connecting_or_starting_a_worker() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().as_os_str().as_bytes();
    let socket = socket_path(profile, &[], uid().unwrap()).unwrap();
    assert!(socket.starts_with(std::env::temp_dir()));
    assert!(!socket.exists());
    for (query, local, shared, filter) in [
        ("the and of", true, false, None),
        (
            "wie konfiguriere ich den gemeinsamen Suchindex",
            true,
            false,
            None,
        ),
        ("cbioportal", true, true, None),
        (
            "cbioportal",
            true,
            false,
            Some(crate::search_filter::Filter::Project(vec![String::new()])),
        ),
    ] {
        let request = Request {
            paths: crate::core::Paths {
                config: None,
                db: None,
                models_dir: None,
            },
            operation: crate::core::Operation::Search {
                query: query.into(),
                intent: None,
                limit: Some(5),
                unit: Some(crate::search_results::SearchUnit::Threads),
                filters: Box::new(crate::search_filter::Selection {
                    filter,
                    ..Default::default()
                }),
                local,
                shared,
            },
        };
        let error = search(&request, profile, &[]).await.unwrap().unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::Usage);
        assert_eq!(error.code, 2);
        assert!(!socket.exists());
    }
}

#[tokio::test]
async fn worker_rejects_mutations_and_exits_after_idle_without_leaving_socket() {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join("test.sock");
    let socket = path.clone();
    let task = tokio::spawn(async move { serve(&socket, Duration::from_millis(150)).await });
    let owner = uid().unwrap();
    let mut stream = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(stream) = connect(&path, owner).await {
                break stream;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let trace_id = TraceId::generate().unwrap();
    let request = mutation(Some(trace_id.clone()));
    write_frame(
        &mut stream,
        &serde_json::to_vec(&request).unwrap(),
        MAX_REQUEST,
    )
    .await
    .unwrap();
    let reply: Reply =
        serde_json::from_slice(&read_frame(&mut stream, MAX_RESPONSE).await.unwrap()).unwrap();
    assert_eq!(reply.code, 2);
    assert!(report_diagnostics(&reply, Some(&trace_id)));
    assert_eq!(
        reply.result.unwrap_err().error.kind,
        crate::ErrorKind::Usage
    );
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!path.exists());
}

#[tokio::test]
async fn frames_reject_oversized_inputs_before_allocating() {
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    writer.write_u32(1024).await.unwrap();
    assert!(read_frame(&mut reader, 10).await.is_err());
}

#[tokio::test]
async fn private_directory_rejects_symlinks_and_broad_permissions() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("worker");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    let owner = uid().unwrap();
    assert!(private_directory(&directory, owner).is_err());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    private_directory(&directory, owner).unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&directory, &link).unwrap();
    assert!(private_directory(&link, owner).is_err());
}
