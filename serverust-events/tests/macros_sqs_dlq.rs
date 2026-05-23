//! Testes da macro `#[subscriber(..., retry = exponential(max = N, base = "T"), dlq = "...")]` (US-008).
//!
//! Garante:
//! - A macro aceita `retry = exponential(max = N, base = "<ms>ms")` e `dlq = "..."`.
//! - Constantes associadas `RETRY_MAX_ATTEMPTS`, `RETRY_BASE_MS` e `DLQ_QUEUE`
//!   ficam disponíveis no struct emitido.
//! - `register` encadeia `EventRouter::with_retry` / `with_dlq` conforme o atributo.
//! - Subscriber sem `retry`/`dlq` continua compilando com defaults
//!   (max=1, base=0, dlq=None) — back-compat.

#![cfg(feature = "in-memory")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serverust_events::broker::in_memory::InMemoryBroker;
use serverust_events::broker::{Broker, BrokerError};
use serverust_events::router::EventRouter;
use serverust_macros::subscriber;

#[subscriber(
    driver = "sqs",
    queue = "orders",
    retry = exponential(max = 5, base = "100ms"),
    dlq = "orders-dlq"
)]
async fn process_order_with_dlq(_event: serde_json::Value) -> Result<(), BrokerError> {
    Ok(())
}

#[subscriber(driver = "sqs", queue = "audit")]
async fn process_audit_no_dlq(_event: serde_json::Value) -> Result<(), BrokerError> {
    Ok(())
}

static MACRO_RETRY_FLAKY_COUNT: AtomicU32 = AtomicU32::new(0);

#[subscriber(
    driver = "sqs",
    queue = "retry-queue",
    retry = exponential(max = 3, base = "1ms")
)]
async fn macro_retry_flaky(_event: serde_json::Value) -> Result<(), BrokerError> {
    let n = MACRO_RETRY_FLAKY_COUNT.fetch_add(1, Ordering::SeqCst);
    if n < 2 {
        Err(BrokerError::Subscribe("simulated".into()))
    } else {
        Ok(())
    }
}

static MACRO_DLQ_FAIL_COUNT: AtomicU32 = AtomicU32::new(0);

#[subscriber(
    driver = "sqs",
    queue = "dlq-src",
    retry = exponential(max = 2, base = "1ms"),
    dlq = "dlq-dst"
)]
async fn macro_dlq_always_fail(_event: serde_json::Value) -> Result<(), BrokerError> {
    MACRO_DLQ_FAIL_COUNT.fetch_add(1, Ordering::SeqCst);
    Err(BrokerError::Subscribe("fail".into()))
}

#[subscriber(driver = "sqs", queue = "only-dlq-q", dlq = "only-dlq-target")]
async fn macro_only_dlq_fail(_event: serde_json::Value) -> Result<(), BrokerError> {
    Err(BrokerError::Subscribe("once".into()))
}

#[test]
fn subscriber_emits_retry_max_attempts_constant() {
    assert_eq!(process_order_with_dlq::RETRY_MAX_ATTEMPTS, 5);
}

#[test]
fn subscriber_emits_retry_base_ms_constant() {
    assert_eq!(process_order_with_dlq::RETRY_BASE_MS, 100);
}

#[test]
fn subscriber_emits_dlq_queue_constant() {
    assert_eq!(process_order_with_dlq::DLQ_QUEUE, Some("orders-dlq"));
}

#[test]
fn subscriber_without_retry_dlq_defaults() {
    assert_eq!(process_audit_no_dlq::RETRY_MAX_ATTEMPTS, 1);
    assert_eq!(process_audit_no_dlq::RETRY_BASE_MS, 0);
    assert_eq!(process_audit_no_dlq::DLQ_QUEUE, None);
}

#[tokio::test]
async fn register_aplica_retry_exponential_do_atributo() {
    MACRO_RETRY_FLAKY_COUNT.store(0, Ordering::SeqCst);
    let broker = Arc::new(InMemoryBroker::new());
    macro_retry_flaky::register(EventRouter::new())
        .attach(broker.clone())
        .await
        .unwrap();
    broker.publish("retry-queue", b"{}").await.unwrap();
    assert_eq!(MACRO_RETRY_FLAKY_COUNT.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn register_aplica_dlq_apos_esgotar_retry() {
    MACRO_DLQ_FAIL_COUNT.store(0, Ordering::SeqCst);
    let broker = Arc::new(InMemoryBroker::new());
    macro_dlq_always_fail::register(EventRouter::new())
        .attach(broker.clone())
        .await
        .unwrap();
    assert!(broker.publish("dlq-src", b"{}").await.is_err());
    assert_eq!(MACRO_DLQ_FAIL_COUNT.load(Ordering::SeqCst), 2);
    let dlq_msgs = broker.messages("dlq-dst");
    assert_eq!(dlq_msgs.len(), 1);
    assert_eq!(dlq_msgs[0].payload, b"{}".to_vec());
}

#[tokio::test]
async fn register_apenas_dlq_sem_exponential() {
    let broker = Arc::new(InMemoryBroker::new());
    macro_only_dlq_fail::register(EventRouter::new())
        .attach(broker.clone())
        .await
        .unwrap();
    assert!(broker
        .publish("only-dlq-q", br#"{"x":1}"#)
        .await
        .is_err());
    let dlq_msgs = broker.messages("only-dlq-target");
    assert_eq!(dlq_msgs.len(), 1);
    assert_eq!(dlq_msgs[0].payload, br#"{"x":1}"#.to_vec());
}
