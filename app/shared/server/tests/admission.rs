//! Authentication and mutation backpressure through the real HTTP router.
use super::*;

#[tokio::test]
async fn busy_mutations_are_rejected_without_blocking_read_routes() {
    let service = Service::with_parameters(false, 384, 64 * 1024 * 1024, FIRST).await;
    // This fixture deliberately has no database. Busy mutations must be rejected
    // before backend access; reads still reach it and return their own failure.
    let permit = service.uploads.clone().acquire_many_owned(4).await.unwrap();
    let index = service.upload(FIRST, &snapshot(1)).await;
    assert_eq!(index.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error: AppError = index.json().await.unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
    assert!(error.error.retryable);
    assert!(error.error.message.contains("mutation capacity"));
    assert_eq!(
        service.upload("invalid", &snapshot(1)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = reqwest::Client::new()
        .get(format!("{}/v1/freshness", service.url))
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
