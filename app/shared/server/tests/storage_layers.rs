use super::*;

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_PG_URL with pgvector"]
async fn postgres_silver_survives_gold_replacement_and_legacy_migration() {
    let service = Service::new(true).await;
    assert_eq!(
        service.upload(FIRST, &snapshot(1)).await.status(),
        StatusCode::OK
    );
    let client = service.pool.get().await.unwrap();
    let schema = backend::group_schema(&service.group);
    client.batch_execute(&format!("SET search_path={schema},public;
        CREATE TEMP TABLE expected_silver AS SELECT * FROM messages;
        CREATE TEMP TABLE expected_gold AS SELECT * FROM message_search;
        ALTER TABLE messages ADD COLUMN generation TEXT, ADD COLUMN vector public.vector,
            ADD COLUMN document TSVECTOR GENERATED ALWAYS AS (to_tsvector('simple',projection)) STORED;
        UPDATE messages m SET generation=g.generation,vector=g.vector FROM message_search g WHERE g.node_id=m.node_id AND g.id=m.id;
        DROP TABLE message_search; UPDATE version SET version=15;")).await.unwrap();
    backend::initialize(&service.pool, std::slice::from_ref(&service.group))
        .await
        .unwrap();
    // Startup is idempotent after the old mixed table has been migrated.
    backend::initialize(&service.pool, std::slice::from_ref(&service.group))
        .await
        .unwrap();
    let unchanged = client.query_one("SELECT NOT EXISTS(SELECT * FROM messages EXCEPT SELECT * FROM expected_silver) AND NOT EXISTS(SELECT * FROM expected_silver EXCEPT SELECT * FROM messages)", &[]).await.unwrap();
    assert!(unchanged.get::<_, bool>(0));
    let preserved = client.query_one("SELECT NOT EXISTS(SELECT * FROM message_search EXCEPT SELECT * FROM expected_gold) AND NOT EXISTS(SELECT * FROM expected_gold EXCEPT SELECT * FROM message_search)", &[]).await.unwrap();
    assert!(preserved.get::<_, bool>(0));
    client
        .execute("DELETE FROM message_search", &[])
        .await
        .unwrap();
    let unchanged = client
        .query_one("SELECT count(*) FROM messages", &[])
        .await
        .unwrap();
    assert_eq!(unchanged.get::<_, i64>(0), 3);
    // Canonical viewing uses silver without a join to derived state.
    let viewed = service
        .request("view", FIRST)
        .json(&json!({"node_id":"first","id":"searchable","context":0}))
        .send()
        .await
        .unwrap();
    assert_eq!(viewed.status(), StatusCode::OK);
    let mut vector = vec![0.0; 384];
    vector[0] = 1.0;
    let query = json!({"query":"hello","vector":vector});
    let unready = service
        .request("search", FIRST)
        .json(&query)
        .send()
        .await
        .unwrap();
    assert_eq!(unready.status(), StatusCode::SERVICE_UNAVAILABLE);
    // Regenerate lexical state from silver; fixture vectors stand in for the
    // node-side model builder, without updating any canonical message row.
    client.execute("INSERT INTO message_search SELECT m.node_id,m.id,g.generation,g.vector,to_tsvector('simple',m.projection) FROM messages m JOIN expected_gold g USING(node_id,id)", &[]).await.unwrap();
    let found = service
        .request("search", FIRST)
        .json(&query)
        .send()
        .await
        .unwrap();
    assert_eq!(found.status(), StatusCode::OK);
    let found: Value = found.json().await.unwrap();
    assert_eq!(found["lexical"][0]["id"], "searchable");
    assert_eq!(found["semantic"][0]["id"], "searchable");
    client
        .execute("DELETE FROM messages WHERE id='searchable'", &[])
        .await
        .unwrap();
    let gold = client
        .query_one(
            "SELECT count(*) FROM message_search WHERE id='searchable'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(gold.get::<_, i64>(0), 0);
    drop(client);
    service.cleanup().await;
}
