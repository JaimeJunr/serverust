//! Testa a trait `IdempotencyStore` via `InMemoryIdempotencyStore`.

use serverust_telemetry::{IdempotencyRecord, IdempotencyStore, InMemoryIdempotencyStore};

#[tokio::test]
async fn in_memory_store_roundtrip() {
    let store = InMemoryIdempotencyStore::new();

    assert!(store.get("missing").await.unwrap().is_none());

    let record = IdempotencyRecord {
        key: "abc".into(),
        response_body: b"{\"ok\":true}".to_vec(),
        status_code: 200,
        created_at_ms: 123,
    };
    store.put(record.clone()).await.unwrap();

    let fetched = store.get("abc").await.unwrap().expect("registro presente");
    assert_eq!(fetched.key, "abc");
    assert_eq!(fetched.status_code, 200);
    assert_eq!(fetched.response_body, b"{\"ok\":true}".to_vec());
    assert_eq!(fetched.created_at_ms, 123);
}

#[tokio::test]
async fn put_overwrites_existing_record() {
    let store = InMemoryIdempotencyStore::new();
    store
        .put(IdempotencyRecord {
            key: "k".into(),
            response_body: vec![1],
            status_code: 200,
            created_at_ms: 1,
        })
        .await
        .unwrap();
    store
        .put(IdempotencyRecord {
            key: "k".into(),
            response_body: vec![2],
            status_code: 202,
            created_at_ms: 2,
        })
        .await
        .unwrap();

    let fetched = store.get("k").await.unwrap().unwrap();
    assert_eq!(fetched.status_code, 202);
    assert_eq!(fetched.response_body, vec![2]);
}
