use super::*;
use futures_util::StreamExt;
use std::time::Duration;

async fn listen(service: &Service, token: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("{}/v1/events", service.url))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
}

/// Reads the stream until an event is complete, or says there was none.
async fn event(response: reqwest::Response, wait: Duration) -> Option<Value> {
    let mut body = response.bytes_stream();
    let mut text = String::new();
    let read = async {
        loop {
            if let Some(data) = text.lines().find_map(|line| line.strip_prefix("data:"))
                && text.contains("\n\n")
            {
                return serde_json::from_str(data.trim()).unwrap();
            }
            let chunk = body.next().await.expect("the stream ended").unwrap();
            text.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    };
    tokio::time::timeout(wait, read).await.ok()
}

// The announcement itself is made where publications commit silver, which
// needs a views backend; the Qdrant-backed publication tests pass through it.
#[tokio::test]
async fn an_announcement_reaches_its_group_as_chat_ids_without_text() {
    let service = Service::canonical().await;
    let stranger = listen(&service, "wrong").await;
    assert_eq!(stranger.status(), StatusCode::UNAUTHORIZED);
    let peer = listen(&service, SECOND).await;
    assert_eq!(peer.status(), StatusCode::OK);
    assert_eq!(peer.headers()["content-type"], "text/event-stream");
    assert!(peer.headers().get("content-encoding").is_none());
    let outsider = listen(&service, OTHER).await;
    assert_eq!(outsider.status(), StatusCode::OK);
    let hub = service.store.events();
    // Nothing changed, so nothing is said.
    hub.announce(&service.first, 1, std::iter::empty(), false);
    hub.announce(
        &service.first,
        2,
        ["conversation".to_owned()].into_iter(),
        true,
    );
    let change = event(peer, Duration::from_secs(20))
        .await
        .expect("the group's listener hears the publication");
    assert_eq!(
        change,
        json!({"node": "first", "revision": 2, "chats": ["conversation"], "all": false, "records": true})
    );
    // Another group's listener hears nothing of it.
    assert_eq!(event(outsider, Duration::from_millis(500)).await, None);
}

#[tokio::test]
async fn too_many_changed_chats_are_not_named() {
    let service = Service::canonical().await;
    let listener = listen(&service, FIRST).await;
    let chats = (0..=100).map(|index| format!("chat-{index}"));
    service
        .store
        .events()
        .announce(&service.first, 5, chats, false);
    let change = event(listener, Duration::from_secs(20)).await.unwrap();
    assert_eq!(change["all"], true);
    assert_eq!(change["chats"], json!([]));
}

#[tokio::test]
async fn stopping_the_server_ends_open_streams() {
    let service = Service::canonical().await;
    let mut body = listen(&service, FIRST).await.bytes_stream();
    service.store.events().stop();
    let ended = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(Ok(_)) = body.next().await {}
    })
    .await;
    assert!(ended.is_ok());
}
