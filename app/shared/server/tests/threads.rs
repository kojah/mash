use super::*;
use crate::thread_catalog::{Direction, Page, Request, Sort};

fn catalog_snapshot(revision: i64) -> Vec<Value> {
    let mut events = snapshot(revision);
    events[1]["records"] = json!([
        {"id":"a","provider":"codex","title":"Alpha","created_at":1000,"updated_at":3000},
        {"id":"b","provider":"claude-code","title":"Beta","created_at":2000,"updated_at":2000},
        {"id":"c","provider":"codex","title":"Alpha","created_at":1000,"updated_at":3000},
        {"id":"d","provider":"hermes","title":"100%_literal","created_at":3000,"updated_at":1000},
        {"id":"e","provider":"codex","title":null,"created_at":null,"updated_at":null}
    ]);
    events.remove(2); // Metadata-only conversations deliberately have no messages or vectors.
    events
}

async fn page(service: &Service, token: &str, input: &Request) -> Page {
    service
        .request("threads", token)
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
            assert!(
                result
                    .threads
                    .iter()
                    .all(|thread| thread.node_id == "first")
            );
            ids.extend(
                result
                    .threads
                    .into_iter()
                    .map(|thread| thread.conversation_id),
            );
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
    assert_eq!(filtered.threads.len(), 2);
    assert_eq!(
        page(service, OTHER, &Request::default())
            .await
            .threads
            .len(),
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
    assert_eq!(peer.threads.len(), 5);
    assert!(peer.threads.iter().all(|thread| thread.node_id == "second"));
    let nonexistent = page(
        service,
        FIRST,
        &Request {
            node: vec!["unknown".into()],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(nonexistent.threads.len(), 0);
    let literal = page(
        service,
        FIRST,
        &Request {
            title_contains: Some("%_".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(literal.threads[0].conversation_id, "d");
}

#[tokio::test]
#[ignore = "requires MASH_TEST_PG_URL with pgvector"]
async fn postgres_thread_catalog_http_filter_sort_page_and_group_contract() {
    contract(Service::new_backend(true, false).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_REDIS_URL with Redis Search"]
async fn redis_thread_catalog_http_filter_sort_page_and_group_contract() {
    contract(Service::new_backend(false, true).await).await;
}

#[tokio::test]
async fn thread_catalog_auth_and_invalid_requests_fail_before_backend_access() {
    let service = Service::new(false).await;
    let unauthorized = service
        .request("threads", "wrong")
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
            .request("threads", FIRST)
            .json(&invalid)
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), expected);
    }
}
