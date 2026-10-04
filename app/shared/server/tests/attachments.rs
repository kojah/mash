use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

async fn call(
    service: &Service,
    route: &str,
    token: &str,
    input: &impl serde::Serialize,
) -> reqwest::Response {
    service
        .request(route, token)
        .json(input)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with disposable Qdrant 1.19+"]
async fn qdrant_http_attachments_require_current_membership_and_group_authorization() {
    let service = Service::qdrant(384).await;
    let source = crate::attachments::Source::Base64 {
        name: "notes.txt".into(),
        media_type: "text/plain".into(),
        data: STANDARD.encode("password=\"hunter2\"\nuseful notes"),
    };
    let cache = tempfile::tempdir().unwrap();
    let record = crate::attachments::retain(
        cache.path(),
        &crate::attachments::Configuration::default(),
        "searchable",
        &[source],
    )
    .unwrap()
    .pop()
    .unwrap();
    let mut events = snapshot(1);
    events[2]["records"][0]["attachments"] = serde_json::to_value(vec![&record]).unwrap();
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::OK
    );
    let request = json!({"node_id":"first","message_id":"searchable","attachment_id":record.id});
    let listed: Value = service
        .request("attachments", SECOND)
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed["attachments"][0]["availability"], "not_uploaded");
    let file = crate::attachments::Retrieved {
        message_id: "searchable".into(),
        attachment: record.clone(),
        data: crate::attachments::read(cache.path(), &record).unwrap(),
    };
    assert_eq!(
        call(&service, "attachment-upload", SECOND, &file)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&service, "attachment-upload", FIRST, &file)
            .await
            .status(),
        StatusCode::OK
    );
    let downloaded: crate::attachments::Retrieved = service
        .request("attachment", SECOND)
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let text = String::from_utf8(STANDARD.decode(&downloaded.data).unwrap()).unwrap();
    assert!(!text.contains("hunter2"));
    assert!(text.contains("useful notes"));
    assert_eq!(
        call(&service, "attachment", "invalid", &request)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&service, "attachment", OTHER, &request).await.status(),
        StatusCode::NOT_FOUND
    );
    let mut corrupted = serde_json::to_value(&file).unwrap();
    corrupted["data"] = json!(STANDARD.encode("tampered"));
    assert_eq!(
        call(&service, "attachment-upload", FIRST, &corrupted)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    // Metadata is in silver snapshots, so immutable replacement removes access even
    // while a deduplicated blob still exists on disk.
    events[0]["revision"] = json!(2);
    events[2]["records"][0]["attachments"] = json!([]);
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        call(&service, "attachment", SECOND, &request)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    service.cleanup().await;
}
