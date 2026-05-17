//! Testa o protocolo de lock InProgress/Completed da trait `IdempotencyStore`
//! via `InMemoryIdempotencyStore` (referência atômica), cobrindo os paths
//! happy / conflict / expired exigidos por US-007.

use serverust_telemetry::idempotency::{
    AcquireOutcome, IdempotencyState, IdempotencyStore, InMemoryIdempotencyStore,
};

const TTL_MS: u64 = 60_000;

#[tokio::test]
async fn try_acquire_returns_acquired_for_new_key() {
    let store = InMemoryIdempotencyStore::new();
    let outcome = store.try_acquire("new-key", 1_000, TTL_MS).await.unwrap();
    assert!(matches!(outcome, AcquireOutcome::Acquired));
}

#[tokio::test]
async fn try_acquire_returns_in_progress_when_locked_within_ttl() {
    let store = InMemoryIdempotencyStore::new();
    // 1ª aquisição grava InProgress.
    let first = store.try_acquire("k", 1_000, TTL_MS).await.unwrap();
    assert!(matches!(first, AcquireOutcome::Acquired));
    // 2ª aquisição dentro do TTL deve ver o lock InProgress.
    let second = store.try_acquire("k", 2_000, TTL_MS).await.unwrap();
    match second {
        AcquireOutcome::InProgress => {}
        other => panic!("esperava InProgress, recebi {other:?}"),
    }
}

#[tokio::test]
async fn try_acquire_returns_already_completed_after_complete_within_ttl() {
    let store = InMemoryIdempotencyStore::new();
    store.try_acquire("k", 1_000, TTL_MS).await.unwrap();
    store.complete("k", 1_500, TTL_MS).await.unwrap();
    let outcome = store.try_acquire("k", 2_000, TTL_MS).await.unwrap();
    match outcome {
        AcquireOutcome::AlreadyCompleted(record) => {
            assert_eq!(record.key, "k");
            assert_eq!(record.state, IdempotencyState::Completed);
            assert_eq!(record.expires_at_ms, 1_500 + TTL_MS);
        }
        other => panic!("esperava AlreadyCompleted, recebi {other:?}"),
    }
}

#[tokio::test]
async fn try_acquire_overrides_expired_in_progress_record() {
    let store = InMemoryIdempotencyStore::new();
    // 1ª aquisição com TTL curto.
    let outcome = store.try_acquire("k", 1_000, 100).await.unwrap();
    assert!(matches!(outcome, AcquireOutcome::Acquired));
    // Passa o tempo de TTL — segunda aquisição deve sobrescrever.
    let outcome = store.try_acquire("k", 1_500, TTL_MS).await.unwrap();
    assert!(
        matches!(outcome, AcquireOutcome::Acquired),
        "expirou — deve sobrescrever",
    );
}

#[tokio::test]
async fn try_acquire_overrides_expired_completed_record() {
    let store = InMemoryIdempotencyStore::new();
    store.try_acquire("k", 1_000, 100).await.unwrap();
    store.complete("k", 1_050, 100).await.unwrap();
    // Após expirar, nova aquisição deve ser concedida (overwrite).
    let outcome = store.try_acquire("k", 5_000, TTL_MS).await.unwrap();
    assert!(matches!(outcome, AcquireOutcome::Acquired));
}

#[tokio::test]
async fn complete_marks_record_as_completed() {
    let store = InMemoryIdempotencyStore::new();
    store.try_acquire("k", 1_000, TTL_MS).await.unwrap();
    store.complete("k", 1_500, TTL_MS).await.unwrap();
    let outcome = store.try_acquire("k", 1_600, TTL_MS).await.unwrap();
    match outcome {
        AcquireOutcome::AlreadyCompleted(rec) => {
            assert_eq!(rec.state, IdempotencyState::Completed);
        }
        other => panic!("esperava AlreadyCompleted, recebi {other:?}"),
    }
}
