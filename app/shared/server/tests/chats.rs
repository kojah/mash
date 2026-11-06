use super::*;

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_chat_catalog_http_filter_sort_page_and_group_contract() {
    contract(Service::qdrant(384).await).await;
}
use crate::chat_catalog::{Direction, Page, Request, Sort};

mod integrated;

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_dashboard_catalog_and_pages_preserve_group_scope() {
    let service = Service::qdrant(384).await;
    assert_eq!(
        service.upload(FIRST, &snapshot(1)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        service.upload(SECOND, &snapshot(1)).await.status(),
        StatusCode::OK
    );
    let catalog = service.get("catalog", FIRST).await;
    assert_eq!(catalog.as_array().unwrap().len(), 1);
    assert_eq!(catalog[0]["node_id"], "second");
    let catalog = service.get("catalog", OTHER).await;
    assert_eq!(catalog, json!([]));
    let input = json!({"node_id":"first","chat_id":"conversation","offset":1,"limit":1});
    let page: Value = service
        .request("chat", SECOND)
        .json(&input)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page.as_array().unwrap().len(), 1);
    assert_eq!(page[0]["id"], "opposite");
    let page: Value = service
        .request("chat", OTHER)
        .json(&input)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page, json!([]));
    for limit in [0, 501] {
        let mut invalid = input.clone();
        invalid["limit"] = json!(limit);
        assert_eq!(
            service
                .request("chat", FIRST)
                .json(&invalid)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        service
            .request("chat", "wrong-token")
            .json(&input)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    service.cleanup().await;
}

fn catalog_snapshot(revision: i64) -> Vec<Value> {
    let mut events = snapshot(revision);
    events[1]["records"] = json!([
        {"id":"a","provider":"codex","title":"Alpha","created_at":1000,"updated_at":3000},
        {"id":"b","provider":"claude-code","title":"Beta","created_at":2000,"updated_at":2000},
        {"id":"c","provider":"codex","title":"Alpha","created_at":1000,"updated_at":3000},
        {"id":"d","provider":"opencode","title":"100%_literal","created_at":3000,"updated_at":1000},
        {"id":"e","provider":"codex","title":null,"created_at":null,"updated_at":null}
    ]);
    events.remove(2); // Metadata-only chats deliberately have no messages or vectors.
    events
}

async fn page(service: &Service, token: &str, input: &Request) -> Page {
    service
        .request("chats", token)
        .json(input)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn contract(service: Service) {
    assert_eq!(
        service.upload(FIRST, &catalog_snapshot(1)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        service.upload(SECOND, &catalog_snapshot(1)).await.status(),
        StatusCode::OK
    );
    verify_ordering_and_pages(&service).await;
    verify_filters(&service).await;
    service.cleanup().await;
}

async fn verify_ordering_and_pages(service: &Service) {
    for (sort, direction, expected) in [
        (
            Sort::UpdatedAt,
            Direction::Asc,
            vec!["d", "b", "a", "c", "e"],
        ),
        (
            Sort::UpdatedAt,
            Direction::Desc,
            vec!["a", "c", "b", "d", "e"],
        ),
        (
            Sort::CreatedAt,
            Direction::Asc,
            vec!["a", "c", "b", "d", "e"],
        ),
        (
            Sort::CreatedAt,
            Direction::Desc,
            vec!["d", "b", "a", "c", "e"],
        ),
        (Sort::Title, Direction::Asc, vec!["d", "a", "c", "b", "e"]),
        (Sort::Title, Direction::Desc, vec!["b", "a", "c", "d", "e"]),
    ] {
        let mut ids = Vec::new();
        for offset in [0, 2, 4] {
            let result = page(
                service,
                FIRST,
                &Request {
                    sort,
                    direction,
                    limit: 2,
                    offset,
                    ..Default::default()
                },
            )
            .await;
            assert_eq!(
                result.next_offset,
                if offset < 4 { Some(offset + 2) } else { None }
            );
            assert!(result.chats.iter().all(|thread| thread.node_id == "first"));
            ids.extend(result.chats.into_iter().map(|thread| thread.chat_id));
        }
        assert_eq!(ids, expected);
    }
}

async fn verify_filters(service: &Service) {
    let filtered = page(
        service,
        FIRST,
        &Request {
            provider: vec![
                crate::provider::Provider::Codex,
                crate::provider::Provider::ClaudeCode,
            ],
            title_contains: Some("aLPha".into()),
            after: Some(3),
            before: Some(4),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(filtered.chats.len(), 2);
    assert_eq!(
        page(service, OTHER, &Request::default()).await.chats.len(),
        0
    );
    let peer = page(
        service,
        FIRST,
        &Request {
            exclude_self: true,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(peer.chats.len(), 5);
    assert!(peer.chats.iter().all(|thread| thread.node_id == "second"));
    let nonexistent = page(
        service,
        FIRST,
        &Request {
            node: vec!["unknown".into()],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(nonexistent.chats.len(), 0);
    let literal = page(
        service,
        FIRST,
        &Request {
            title_contains: Some("%_".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(literal.chats[0].chat_id, "d");
}

#[tokio::test]
async fn chat_catalog_auth_and_invalid_requests_fail_before_backend_access() {
    let service = Service::new(false).await;
    let unauthorized = service
        .request("chats", "wrong")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    for (invalid, expected) in [
        (json!({"limit":0}), StatusCode::BAD_REQUEST),
        (json!({"offset":10001}), StatusCode::BAD_REQUEST),
        (json!({"sort":"random"}), StatusCode::UNPROCESSABLE_ENTITY),
        (json!({"extra":true}), StatusCode::UNPROCESSABLE_ENTITY),
        (json!({"after":-1}), StatusCode::BAD_REQUEST),
    ] {
        let result = service
            .request("chats", FIRST)
            .json(&invalid)
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), expected);
    }
}
