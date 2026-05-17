//! Testes do `IdempotencyLayer` com semântica InProgress/Completed/TTL
//! (US-007). Cobre happy / conflict / expired paths.

#![cfg(feature = "sqs")]

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use aws_lambda_events::event::sqs::SqsMessage;
use serverust_events::broker::BrokerError;
use serverust_events::sqs::layers::IdempotencyLayer;
use serverust_events::sqs::subscriber::SqsSubscriber;
use serverust_telemetry::InMemoryIdempotencyStore;
use serverust_telemetry::idempotency::{AcquireOutcome, IdempotencyStore};
use tower::{Service, ServiceBuilder, ServiceExt};

fn message_with_id(id: &str) -> SqsMessage {
    let mut m = SqsMessage::default();
    m.message_id = Some(id.to_string());
    m.body = Some("payload".to_string());
    m
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[tokio::test]
async fn happy_path_first_run_acquires_and_completes() {
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Ok::<_, BrokerError>(())
        }
    });

    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::new());

    let mut svc = ServiceBuilder::new()
        .layer(IdempotencyLayer::new(store.clone()).with_ttl(Duration::from_secs(60)))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_id("happy-1")).await.unwrap();

    assert_eq!(*calls.lock().unwrap(), 1, "handler executa uma vez");

    // Store deve estar Completed após sucesso.
    let outcome = store
        .try_acquire("happy-1", now_ms(), 60_000)
        .await
        .unwrap();
    assert!(
        matches!(outcome, AcquireOutcome::AlreadyCompleted(_)),
        "após sucesso, lock fica Completed",
    );
}

#[tokio::test]
async fn skip_when_completed_within_ttl() {
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Ok::<_, BrokerError>(())
        }
    });

    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::new());

    let mut svc = ServiceBuilder::new()
        .layer(IdempotencyLayer::new(store).with_ttl(Duration::from_secs(60)))
        .service(subscriber);

    let msg = message_with_id("dup-key");
    svc.ready().await.unwrap();
    svc.call(msg.clone()).await.unwrap();
    svc.ready().await.unwrap();
    svc.call(msg).await.unwrap();

    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "2ª chamada dentro do TTL deve ser skipada",
    );
}

#[tokio::test]
async fn conflict_when_in_progress_lock_held_by_other_worker() {
    // Simula: outro worker já adquiriu o lock InProgress (não chegou a Completed).
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move { Ok::<_, BrokerError>(()) });

    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::new());
    // Pré-popula lock InProgress.
    let outcome = store.try_acquire("locked", now_ms(), 60_000).await.unwrap();
    assert!(matches!(outcome, AcquireOutcome::Acquired));

    let mut svc = ServiceBuilder::new()
        .layer(IdempotencyLayer::new(store).with_ttl(Duration::from_secs(60)))
        .service(subscriber);

    svc.ready().await.unwrap();
    let result = svc.call(message_with_id("locked")).await;
    let err = result.expect_err("InProgress de outro worker deve causar erro (SQS retentará)");
    assert!(matches!(err, BrokerError::Subscribe(_)));
    assert!(
        err.to_string().to_lowercase().contains("in progress")
            || err.to_string().to_lowercase().contains("in_progress"),
        "mensagem de erro indica InProgress, recebi: {err}",
    );
}

#[tokio::test]
async fn expired_record_allows_reprocessing() {
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Ok::<_, BrokerError>(())
        }
    });

    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::new());

    // Pre-popula com lock expirado (1 ms de TTL no passado).
    let very_old_now = 1_000;
    let outcome = store.try_acquire("expired", very_old_now, 1).await.unwrap();
    assert!(matches!(outcome, AcquireOutcome::Acquired));
    store.complete("expired", very_old_now, 1).await.unwrap();

    let mut svc = ServiceBuilder::new()
        .layer(IdempotencyLayer::new(store).with_ttl(Duration::from_secs(60)))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_id("expired")).await.unwrap();

    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "registro expirado permite reprocessamento",
    );
}

#[tokio::test]
async fn handler_failure_does_not_persist_completed_record() {
    // Após erro, o lock InProgress fica gravado e expira; ele NÃO vira Completed,
    // de modo que uma retentativa após o TTL pode reprocessar.
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Err::<(), _>(BrokerError::Subscribe("boom".into()))
        }
    });

    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::new());

    let mut svc = ServiceBuilder::new()
        .layer(IdempotencyLayer::new(store.clone()).with_ttl(Duration::from_secs(60)))
        .service(subscriber);

    svc.ready().await.unwrap();
    let err = svc.call(message_with_id("fail")).await.unwrap_err();
    assert!(matches!(err, BrokerError::Subscribe(_)));
    assert_eq!(*calls.lock().unwrap(), 1);

    // Após falha, store está InProgress (não Completed) — try_acquire dentro
    // do TTL ainda vê InProgress (lock segura até TTL expirar).
    let outcome = store.try_acquire("fail", now_ms(), 60_000).await.unwrap();
    assert!(
        matches!(outcome, AcquireOutcome::InProgress),
        "falha não deve marcar Completed",
    );
}

#[tokio::test]
async fn without_message_id_falls_through() {
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Ok::<_, BrokerError>(())
        }
    });

    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::new());

    let mut svc = ServiceBuilder::new()
        .layer(IdempotencyLayer::new(store).with_ttl(Duration::from_secs(60)))
        .service(subscriber);

    let mut msg = SqsMessage::default();
    msg.message_id = None;
    msg.body = Some("x".into());

    svc.ready().await.unwrap();
    svc.call(msg.clone()).await.unwrap();
    svc.ready().await.unwrap();
    svc.call(msg).await.unwrap();

    assert_eq!(
        *calls.lock().unwrap(),
        2,
        "sem message_id, IdempotencyLayer é bypass",
    );
}
