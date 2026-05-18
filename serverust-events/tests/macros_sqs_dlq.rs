//! Testes da macro `#[subscriber(..., retry = exponential(max = N, base = "T"), dlq = "...")]` (US-008).
//!
//! Garante:
//! - A macro aceita `retry = exponential(max = N, base = "<ms>ms")` e `dlq = "..."`.
//! - Constantes associadas `RETRY_MAX_ATTEMPTS`, `RETRY_BASE_MS` e `DLQ_QUEUE`
//!   ficam disponíveis no struct emitido.
//! - `register` aplica `EventRouter::with_retry` / `with_dlq` conforme o atributo.
//! - Subscriber sem `retry`/`dlq` continua compilando com defaults
//!   (max=1, base=0, dlq=None) — back-compat.

#![cfg(feature = "in-memory")]

use serverust_events::broker::BrokerError;
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

#[subscriber(driver = "sqs", queue = "retry-only", retry = exponential(max = 4, base = "50ms"))]
async fn process_retry_only(_event: serde_json::Value) -> Result<(), BrokerError> {
    Ok(())
}

#[subscriber(driver = "sqs", queue = "dlq-only", dlq = "audit-dlq")]
async fn process_dlq_only(_event: serde_json::Value) -> Result<(), BrokerError> {
    Ok(())
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

#[test]
fn register_aplica_retry_e_dlq_no_event_router() {
    let router = process_order_with_dlq::register(EventRouter::new());
    assert!(router.last_retry().is_some());
    assert_eq!(router.last_dlq(), Some("orders-dlq"));
}

#[test]
fn register_retry_only_sem_with_dlq() {
    let router = process_retry_only::register(EventRouter::new());
    assert!(router.last_retry().is_some());
    assert_eq!(router.last_dlq(), None);
}

#[test]
fn register_dlq_only_sem_with_retry() {
    let router = process_dlq_only::register(EventRouter::new());
    assert!(router.last_retry().is_none());
    assert_eq!(router.last_dlq(), Some("audit-dlq"));
}
