//! Testes da macro `#[subscriber(driver = "sqs", queue = "...")]` (US-001).
//!
//! Garante:
//! - A macro aceita `driver = "sqs", queue = "..."` e compila.
//! - A constante associada `SUBSCRIBE_TOPIC` traz o nome da fila.
//! - `DRIVER` distingue sqs de kafka.
//! - `register(router)` inscreve o handler para o nome da fila.
//! - `topic = "..."` (legado) continua funcionando como `driver = "kafka"`.

#![cfg(feature = "in-memory")]

use std::sync::Arc;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serverust_events::broker::Broker;
use serverust_events::broker::BrokerError;
use serverust_events::broker::in_memory::InMemoryBroker;
use serverust_events::router::EventRouter;
use serverust_macros::subscriber;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Order {
    #[serde(rename = "orderId")]
    order_id: String,
    amount: u64,
}

static SQS_SINK: Mutex<Vec<Order>> = Mutex::new(Vec::new());

#[subscriber(driver = "sqs", queue = "orders")]
async fn handle_sqs_order(event: Order) -> Result<(), BrokerError> {
    SQS_SINK.lock().unwrap().push(event);
    Ok(())
}

#[test]
fn subscriber_sqs_emite_constantes() {
    assert_eq!(handle_sqs_order::SUBSCRIBE_TOPIC, "orders");
    assert_eq!(handle_sqs_order::DRIVER, "sqs");
    assert_eq!(handle_sqs_order::PUBLISH_TOPIC, None);
}

#[test]
fn subscriber_sqs_register_inscreve_pela_fila() {
    let router = handle_sqs_order::register(EventRouter::new());
    assert_eq!(router.subscription_topics(), vec!["orders".to_string()]);
}

#[tokio::test]
async fn subscriber_sqs_processa_payload_via_in_memory_broker() {
    SQS_SINK.lock().unwrap().clear();
    let broker = Arc::new(InMemoryBroker::new());
    handle_sqs_order::register(EventRouter::new())
        .attach(broker.clone())
        .await
        .unwrap();

    let payload = serde_json::to_vec(&Order {
        order_id: "o-1".into(),
        amount: 5,
    })
    .unwrap();
    broker.publish("orders", &payload).await.unwrap();

    assert_eq!(
        SQS_SINK.lock().unwrap().clone(),
        vec![Order {
            order_id: "o-1".into(),
            amount: 5
        }]
    );
}

// Back-compat: `topic = "..."` continua funcionando como driver = "kafka" implicito.
#[subscriber(topic = "kafka.orders")]
async fn handle_kafka_order(_event: Order) -> Result<(), BrokerError> {
    Ok(())
}

#[test]
fn subscriber_kafka_back_compat_topic_alone() {
    assert_eq!(handle_kafka_order::SUBSCRIBE_TOPIC, "kafka.orders");
    assert_eq!(handle_kafka_order::DRIVER, "kafka");
}
