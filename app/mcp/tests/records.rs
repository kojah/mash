use super::*;

#[tokio::test]
async fn native_records_sdk_mutations_reads_and_writer_admission_need_no_models() {
    let root = tempfile::tempdir().unwrap();
    let paths = super::workflows::decision_history(root.path());
    let database = paths.db.clone().unwrap();
    let server = Mash {
        paths: Arc::new(paths),
        gate: Arc::default(),
        runtime: None,
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let (server_io, client_io) = tokio::io::duplex(128 * 1024);
    let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let service = serving.await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        client.call_tool(
            CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
        )
    };
    let request = json!({"operation_id":"sdk-create","change":{"action":"create","scope":{"kind":"chat","id":"decision-thread"},"body":{"statement":"Use Qdrant","review":"established","recorded_by":"test agent","details":{"kind":"decision","state":"accepted"}},"evidence":[{"message_id":"message-2","quote":"Yes, let's use Qdrant.","purpose":"endorsement"}]}});
    let guard = crate::storage::admission::acquire(&database).unwrap();
    let blocked = call("record", request.clone()).await.unwrap();
    assert_eq!(blocked.is_error, Some(true));
    drop(guard);
    let created = tokio::time::timeout(
        crate::test_support::HANG_GUARD,
        call("record", request.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_ne!(created.is_error, Some(true), "{created:?}");
    let receipt = created.structured_content.unwrap();
    let retry = call("record", request).await.unwrap();
    assert_eq!(retry.structured_content.as_ref(), Some(&receipt));
    let listed = call("list_records", json!({})).await.unwrap();
    assert_ne!(listed.is_error, Some(true), "{listed:?}");
    assert_eq!(
        listed.structured_content.unwrap()["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let history = call(
        "get_record",
        json!({"id":receipt["record_id"],"history":true}),
    )
    .await
    .unwrap();
    assert_eq!(
        history.structured_content.unwrap()["revisions"][0]["evidence_current"],
        true
    );
    let forgotten = call("record",json!({"operation_id":"sdk-forget","change":{"action":"forget","expected":{"id":receipt["record_id"],"revision":receipt["revision"]}}})).await.unwrap();
    assert_ne!(forgotten.is_error, Some(true), "{forgotten:?}");
    assert_eq!(
        call("get_record", json!({"id":receipt["record_id"]}))
            .await
            .unwrap()
            .is_error,
        Some(true)
    );
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
}
