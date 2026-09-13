//! Testa o protocolo de lock InProgress/Completed da trait `IdempotencyStore`
//! via `InMemoryIdempotencyStore` (referência atômica), cobrindo os paths
//! happy / conflict / expired exigidos por US-007.

use serverust_telemetry::idempotency::{
    AcquireOutcome, IdempotencyState, IdempotencyStore, InMemoryIdempotencyStore, LockToken,
};

const TTL_MS: u64 = 60_000;

fn expect_acquired(outcome: AcquireOutcome) -> LockToken {
    match outcome {
        AcquireOutcome::Acquired(token) => token,
        other => panic!("esperava Acquired(token), recebi {other:?}"),
    }
}

#[tokio::test]
async fn try_acquire_returns_acquired_for_new_key() {
    let store = InMemoryIdempotencyStore::new();
    let outcome = store.try_acquire("new-key", 1_000, TTL_MS).await.unwrap();
    assert!(matches!(outcome, AcquireOutcome::Acquired(_)));
}

#[tokio::test]
async fn try_acquire_returns_in_progress_when_locked_within_ttl() {
    let store = InMemoryIdempotencyStore::new();
    // 1ª aquisição grava InProgress.
    let first = store.try_acquire("k", 1_000, TTL_MS).await.unwrap();
    assert!(matches!(first, AcquireOutcome::Acquired(_)));
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
    let token = expect_acquired(store.try_acquire("k", 1_000, TTL_MS).await.unwrap());
    store.complete("k", &token, 1_500, TTL_MS).await.unwrap();
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
    assert!(matches!(outcome, AcquireOutcome::Acquired(_)));
    // Passa o tempo de TTL — segunda aquisição deve sobrescrever.
    let outcome = store.try_acquire("k", 1_500, TTL_MS).await.unwrap();
    assert!(
        matches!(outcome, AcquireOutcome::Acquired(_)),
        "expirou — deve sobrescrever",
    );
}

#[tokio::test]
async fn try_acquire_overrides_expired_completed_record() {
    let store = InMemoryIdempotencyStore::new();
    let token = expect_acquired(store.try_acquire("k", 1_000, 100).await.unwrap());
    store.complete("k", &token, 1_050, 100).await.unwrap();
    // Após expirar, nova aquisição deve ser concedida (overwrite).
    let outcome = store.try_acquire("k", 5_000, TTL_MS).await.unwrap();
    assert!(matches!(outcome, AcquireOutcome::Acquired(_)));
}

#[tokio::test]
async fn release_removes_in_progress_lock() {
    let store = InMemoryIdempotencyStore::new();
    let token = expect_acquired(store.try_acquire("k", 1_000, TTL_MS).await.unwrap());
    store.release("k", &token).await.unwrap();
    let outcome = store.try_acquire("k", 2_000, TTL_MS).await.unwrap();
    assert!(
        matches!(outcome, AcquireOutcome::Acquired(_)),
        "após release, nova aquisição deve ser concedida",
    );
}

#[tokio::test]
async fn release_is_noop_for_completed_record() {
    let store = InMemoryIdempotencyStore::new();
    let token = expect_acquired(store.try_acquire("k", 1_000, TTL_MS).await.unwrap());
    store.complete("k", &token, 1_500, TTL_MS).await.unwrap();
    store.release("k", &token).await.unwrap();
    let outcome = store.try_acquire("k", 2_000, TTL_MS).await.unwrap();
    assert!(matches!(outcome, AcquireOutcome::AlreadyCompleted(_)));
}

#[tokio::test]
async fn release_with_stale_token_does_not_drop_new_owner_lock() {
    let store = InMemoryIdempotencyStore::new();
    let token_a = expect_acquired(store.try_acquire("k", 1_000, 100).await.unwrap());
    let token_b = expect_acquired(store.try_acquire("k", 1_500, TTL_MS).await.unwrap());
    assert_ne!(
        token_a, token_b,
        "nova aquisição deve emitir token distinto"
    );

    store.release("k", &token_a).await.unwrap();

    let outcome = store.try_acquire("k", 1_600, TTL_MS).await.unwrap();
    match outcome {
        AcquireOutcome::InProgress => {}
        other => panic!(
            "release com token obsoleto não deve apagar lock do dono corrente, recebi {other:?}"
        ),
    }

    store.release("k", &token_b).await.unwrap();
    let outcome = store.try_acquire("k", 1_700, TTL_MS).await.unwrap();
    assert!(
        matches!(outcome, AcquireOutcome::Acquired(_)),
        "release com token do dono corrente deve liberar o lock",
    );
}

#[tokio::test]
async fn complete_with_stale_token_does_not_overwrite_new_owner_lock() {
    let store = InMemoryIdempotencyStore::new();
    let token_a = expect_acquired(store.try_acquire("k", 1_000, 100).await.unwrap());
    let token_b = expect_acquired(store.try_acquire("k", 1_500, TTL_MS).await.unwrap());
    assert_ne!(token_a, token_b);

    store.complete("k", &token_a, 1_600, TTL_MS).await.unwrap();

    let outcome = store.try_acquire("k", 1_700, TTL_MS).await.unwrap();
    match outcome {
        AcquireOutcome::InProgress => {}
        AcquireOutcome::AlreadyCompleted(_) => {
            panic!("complete com token obsoleto não deve marcar Completed no lock de outro dono")
        }
        other => panic!("esperava InProgress do dono corrente, recebi {other:?}"),
    }

    store.complete("k", &token_b, 1_800, TTL_MS).await.unwrap();
    let outcome = store.try_acquire("k", 1_900, TTL_MS).await.unwrap();
    assert!(
        matches!(outcome, AcquireOutcome::AlreadyCompleted(_)),
        "complete com token do dono corrente deve marcar Completed",
    );
}
