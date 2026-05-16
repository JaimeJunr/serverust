//! Testes dos extractors tipados: EventCtx, KafkaHeaders, State<S> (US-4).
//!
//! Cobre todos os ACs:
//! - State<S> injeta estado compartilhado
//! - KafkaHeaders expõe headers do registro
//! - EventCtx expõe topic, partition, offset, timestamp
//! - event: T desserializa payload JSON
//! - todos os extractors funcionam em combinação no mesmo handler

#![cfg(feature = "in-memory")]

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use serde::{Deserialize, Serialize};
use serverust_events::{
    broker::{Broker, in_memory::InMemoryBroker},
    extract::{EventCtx, KafkaHeaders, State},
    router::EventRouter,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TestEvent {
    value: u64,
}

// ---------------------------------------------------------------------------
// AC: State<AppState> injeta estado compartilhado
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_state_extractor_injects_shared_state() {
    let broker = Arc::new(InMemoryBroker::default());
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = counter.clone();

    EventRouter::new()
        .with_state(42u32)
        .subscribe_with("state-topic", move |_event: TestEvent, s: State<u32>| {
            let counter = counter_clone.clone();
            async move {
                counter.fetch_add(*s.0, Ordering::SeqCst);
                Ok(())
            }
        })
        .attach(broker.clone())
        .await
        .unwrap();

    broker
        .publish("state-topic", b"{\"value\":1}")
        .await
        .unwrap();

    assert_eq!(counter.load(Ordering::SeqCst), 42);
}

#[tokio::test]
async fn test_state_extractor_missing_returns_error() {
    let broker = Arc::new(InMemoryBroker::default());

    // Sem with_state — extractor deve retornar BrokerError
    EventRouter::new()
        .subscribe_with(
            "no-state-topic",
            |_event: TestEvent, _s: State<u32>| async { Ok(()) },
        )
        .attach(broker.clone())
        .await
        .unwrap();

    let result = broker.publish("no-state-topic", b"{\"value\":1}").await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// AC: KafkaHeaders expõe headers do registro como HashMap
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_kafka_headers_extractor_empty() {
    let broker = Arc::new(InMemoryBroker::default());
    let called = Arc::new(AtomicU32::new(0));
    let called_clone = called.clone();

    EventRouter::new()
        .subscribe_with(
            "headers-topic",
            move |_event: TestEvent, h: KafkaHeaders| {
                let called = called_clone.clone();
                async move {
                    assert!(h.0.is_empty());
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .attach(broker.clone())
        .await
        .unwrap();

    broker
        .publish("headers-topic", b"{\"value\":1}")
        .await
        .unwrap();

    assert_eq!(called.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// AC: EventCtx expõe topic, partition, offset, timestamp
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_event_ctx_extractor_exposes_topic() {
    let broker = Arc::new(InMemoryBroker::default());
    let called = Arc::new(AtomicU32::new(0));
    let called_clone = called.clone();

    EventRouter::new()
        .subscribe_with("ctx-topic", move |_event: TestEvent, ctx: EventCtx| {
            let called = called_clone.clone();
            async move {
                assert_eq!(ctx.topic, "ctx-topic");
                assert!(ctx.partition.is_none());
                assert!(ctx.offset.is_none());
                // timestamp pode ser None no InMemoryBroker
                called.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .attach(broker.clone())
        .await
        .unwrap();

    broker.publish("ctx-topic", b"{\"value\":1}").await.unwrap();

    assert_eq!(called.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// AC: event: T where T: DeserializeOwned desserializa o payload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_event_deserialization() {
    let broker = Arc::new(InMemoryBroker::default());
    let received = Arc::new(AtomicU32::new(0));
    let received_clone = received.clone();

    EventRouter::new()
        .subscribe_with("payload-topic", move |event: TestEvent| {
            let received = received_clone.clone();
            async move {
                received.store(event.value as u32, Ordering::SeqCst);
                Ok(())
            }
        })
        .attach(broker.clone())
        .await
        .unwrap();

    broker
        .publish("payload-topic", b"{\"value\":99}")
        .await
        .unwrap();

    assert_eq!(received.load(Ordering::SeqCst), 99);
}

#[tokio::test]
async fn test_event_deserialization_invalid_payload_returns_error() {
    let broker = Arc::new(InMemoryBroker::default());

    EventRouter::new()
        .subscribe_with("bad-topic", |_event: TestEvent| async { Ok(()) })
        .attach(broker.clone())
        .await
        .unwrap();

    let result = broker.publish("bad-topic", b"not json").await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// AC: todos os extractors funcionam em combinação no mesmo handler
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_all_extractors_combined() {
    let broker = Arc::new(InMemoryBroker::default());
    let called = Arc::new(AtomicU32::new(0));
    let called_clone = called.clone();

    EventRouter::new()
        .with_state("shared".to_string())
        .subscribe_with(
            "combined-topic",
            move |event: TestEvent, ctx: EventCtx, h: KafkaHeaders, s: State<String>| {
                let called = called_clone.clone();
                async move {
                    assert_eq!(event.value, 7);
                    assert_eq!(ctx.topic, "combined-topic");
                    assert!(h.0.is_empty());
                    assert_eq!(s.0.as_str(), "shared");
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
        .attach(broker.clone())
        .await
        .unwrap();

    broker
        .publish("combined-topic", b"{\"value\":7}")
        .await
        .unwrap();

    assert_eq!(called.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// AC: State<S> com tipo errado retorna erro descritivo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_state_type_mismatch_returns_error() {
    let broker = Arc::new(InMemoryBroker::default());

    EventRouter::new()
        .with_state(42u32)
        // Handler espera String, mas estado é u32
        .subscribe_with(
            "mismatch-topic",
            |_event: TestEvent, _s: State<String>| async { Ok(()) },
        )
        .attach(broker.clone())
        .await
        .unwrap();

    let result = broker.publish("mismatch-topic", b"{\"value\":1}").await;
    assert!(result.is_err());
}
