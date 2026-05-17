//! Testes da macro `#[subscriber(..., retry = exponential(max = N, base = "T"), dlq = "...")]` (US-008).
//!
//! Garante:
//! - A macro aceita `retry = exponential(max = N, base = "<ms>ms")` e `dlq = "..."`.
//! - Constantes associadas `RETRY_MAX_ATTEMPTS`, `RETRY_BASE_MS` e `DLQ_QUEUE`
//!   ficam disponíveis no struct emitido.
//! - Subscriber sem `retry`/`dlq` continua compilando com defaults
//!   (max=1, base=0, dlq=None) — back-compat.

#![cfg(feature = "in-memory")]

use serverust_events::broker::BrokerError;
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
