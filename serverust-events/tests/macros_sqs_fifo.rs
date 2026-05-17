//! Testes da macro `#[subscriber(driver = "sqs", queue = "...", fifo)]` (US-005).
//!
//! Garante:
//! - A macro aceita o flag `fifo` e expoe `Self::IS_FIFO == true`.
//! - Subscriber FIFO declara `SqsFifoMetadata` na assinatura.
//! - `register(router)` continua inscrevendo o handler pelo nome da fila.
//! - Subscriber standard (sem `fifo`) tem `IS_FIFO == false`.
//!
//! Casos de compile-fail (sem o flag, sem o extractor, etc.) estao em
//! `serverust-macros/tests/ui/fail_*.rs` (trybuild).

#![cfg(feature = "in-memory")]

use serverust_events::broker::BrokerError;
use serverust_events::router::EventRouter;
use serverust_events::sqs::extract::SqsFifoMetadata;
use serverust_macros::subscriber;

#[subscriber(driver = "sqs", queue = "orders.fifo", fifo)]
async fn handle_fifo_order(
    _event: serde_json::Value,
    meta: SqsFifoMetadata,
) -> Result<(), BrokerError> {
    let _ = meta.message_group_id;
    Ok(())
}

#[subscriber(driver = "sqs", queue = "orders")]
async fn handle_standard_order(_event: serde_json::Value) -> Result<(), BrokerError> {
    Ok(())
}

#[test]
fn fifo_subscriber_emite_is_fifo_true() {
    const { assert!(handle_fifo_order::IS_FIFO) };
    assert_eq!(handle_fifo_order::SUBSCRIBE_TOPIC, "orders.fifo");
    assert_eq!(handle_fifo_order::DRIVER, "sqs");
}

#[test]
fn standard_subscriber_emite_is_fifo_false() {
    const { assert!(!handle_standard_order::IS_FIFO) };
}

#[test]
fn fifo_subscriber_register_inscreve_pela_fila() {
    let router = handle_fifo_order::register(EventRouter::new());
    assert_eq!(
        router.subscription_topics(),
        vec!["orders.fifo".to_string()]
    );
}
