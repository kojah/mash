//! Authentication and mutation backpressure through the real HTTP router.
use super::*;

#[tokio::test]
async fn index_and_forget_share_capacity_without_blocking_read_routes() {
    let service = Service::with_parameters(false, 384, 64 * 1024 * 1024, FIRST).await;
    // This fixture deliberately has no database. Busy mutations must be rejected
    // before backend access; reads still reach it and return their own failure.
    let permit = service.uploads.clone().acquire_many_owned(4).await.unwrap();
    let forget = protocol::Forget {
        instance: "instance".into(),
        revision: None,
        ids: vec![],
    };
    let index = service.upload(FIRST, &snapshot(1)).await;
    let deletion = service
        .request("forget", FIRST)
        .json(&forget)
        .send()
        .await
        .unwrap();
    for response in [index, deletion] {
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let error: AppError = response.json().await.unwrap();
        assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
        assert!(error.error.retryable);
        assert!(error.error.message.contains("mutation capacity"));
    }
    assert_eq!(
        service
            .request("forget", "invalid")
            .json(&forget)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = reqwest::Client::new()
        .get(format!("{}/v1/status", service.url))
        .bearer_auth(FIRST)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error: AppError = response.json().await.unwrap();
    assert!(!error.error.message.contains("mutation capacity"));
    drop(permit);
    assert_eq!(service.uploads.available_permits(), 4);
    service.task.abort();
}
