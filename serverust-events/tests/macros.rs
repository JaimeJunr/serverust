//! Testes das macros `#[subscriber]` e `#[publisher]` (US-6).
//!
//! Cobre:
//! - `#[subscriber(topic = "...")]` em ack-only (`-> Result<(), BrokerError>`)
//!   compila e registra o handler no `EventRouter` via builder programático.
//! - Handler com `event: T` recebe payload JSON decodificado automaticamente.
//! - `#[publisher(topic = "...")]` empilhável serializa o `Ok(U)` e publica.
//! - `-> Result<(), BrokerError>` (sem `#[publisher]`) é ack-only — nada é
//!   publicado.
//! - As macros emitem código do builder `EventRouter` (constantes + chamada
//!   direta a `subscribe`/`subscribe_publish`), sem registro global de runtime.

#![cfg(feature = "in-memory")]

use std::sync::Arc;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serverust_events::broker::in_memory::InMemoryBroker;
use serverust_events::broker::{Broker, BrokerError};
use serverust_events::router::EventRouter;
// `publisher` é consumido pelo `subscriber` e nunca expande sozinho — mas a
// macro precisa estar em scope para que o `#[publisher(...)]` empilhado seja
// resolvido pelo compilador antes de o `#[subscriber]` mais externo rodar.
#[allow(unused_imports)]
use serverust_macros::publisher;
use serverust_macros::subscriber;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct OrderCreated {
    id: u64,
    total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct OrderConfirmed {
    id: u64,
}

// ---------------------------------------------------------------------------
// Subscriber ack-only — captura os eventos recebidos em um sink global.
// ---------------------------------------------------------------------------

static ACK_SINK: Mutex<Vec<OrderCreated>> = Mutex::new(Vec::new());

#[subscriber(topic = "orders.created")]
async fn handle_order_ack(event: OrderCreated) -> Result<(), BrokerError> {
    ACK_SINK.lock().unwrap().push(event);
    Ok(())
}

#[test]
fn subscriber_emite_constantes_de_topico() {
    assert_eq!(handle_order_ack::SUBSCRIBE_TOPIC, "orders.created");
    assert_eq!(handle_order_ack::PUBLISH_TOPIC, None);
}

#[test]
fn subscriber_emite_register_que_inscreve_no_router() {
    let router = handle_order_ack::register(EventRouter::new());
    assert_eq!(
        router.subscription_topics(),
        vec!["orders.created".to_string()]
    );
    assert!(router.last_publish_topic().is_none());
}

#[tokio::test]
async fn subscriber_recebe_payload_decodificado_via_in_memory() {
    ACK_SINK.lock().unwrap().clear();
    let broker = Arc::new(InMemoryBroker::new());
    handle_order_ack::register(EventRouter::new())
        .attach(broker.clone())
        .await
        .unwrap();

    let payload = serde_json::to_vec(&OrderCreated { id: 1, total: 100 }).unwrap();
    broker.publish("orders.created", &payload).await.unwrap();

    let got = ACK_SINK.lock().unwrap().clone();
    assert_eq!(got, vec![OrderCreated { id: 1, total: 100 }]);
}

// ---------------------------------------------------------------------------
// Subscriber + publisher empilhados — publica o valor de retorno no tópico
// configurado em `#[publisher]`.
// ---------------------------------------------------------------------------

#[subscriber(topic = "orders.created")]
#[publisher(topic = "orders.confirmed")]
async fn handle_order_with_publish(event: OrderCreated) -> Result<OrderConfirmed, BrokerError> {
    Ok(OrderConfirmed { id: event.id })
}

#[test]
fn publisher_empilhado_expoe_topico_em_constante() {
    assert_eq!(handle_order_with_publish::SUBSCRIBE_TOPIC, "orders.created");
    assert_eq!(
        handle_order_with_publish::PUBLISH_TOPIC,
        Some("orders.confirmed")
    );
}

#[test]
fn publisher_empilhado_registra_via_subscribe_publish() {
    let router = handle_order_with_publish::register(EventRouter::new());
    assert_eq!(
        router.subscription_topics(),
        vec!["orders.created".to_string()]
    );
    assert_eq!(router.last_publish_topic(), Some("orders.confirmed"));
}

#[tokio::test]
async fn publisher_serializa_e_publica_valor_de_retorno() {
    let broker = Arc::new(InMemoryBroker::new());
    handle_order_with_publish::register(EventRouter::new())
        .attach(broker.clone())
        .await
        .unwrap();

    let payload = serde_json::to_vec(&OrderCreated { id: 7, total: 42 }).unwrap();
    broker.publish("orders.created", &payload).await.unwrap();

    let confirmed = broker.messages("orders.confirmed");
    assert_eq!(confirmed.len(), 1);
    let parsed: OrderConfirmed = serde_json::from_slice(&confirmed[0].payload).unwrap();
    assert_eq!(parsed, OrderConfirmed { id: 7 });
}

// ---------------------------------------------------------------------------
// Subscriber ack-only NÃO publica no tópico de saída (não houve `#[publisher]`).
// ---------------------------------------------------------------------------

#[subscriber(topic = "orders.silent")]
async fn handle_silent(_event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}

#[tokio::test]
async fn subscriber_sem_publisher_nao_publica_em_nenhum_topico() {
    let broker = Arc::new(InMemoryBroker::new());
    handle_silent::register(EventRouter::new())
        .attach(broker.clone())
        .await
        .unwrap();

    let payload = serde_json::to_vec(&OrderCreated { id: 1, total: 10 }).unwrap();
    broker.publish("orders.silent", &payload).await.unwrap();

    // Apenas o tópico de entrada teve mensagem; nenhum outro tópico foi escrito.
    assert_eq!(broker.messages("orders.silent").len(), 1);
    assert!(broker.messages("orders.confirmed").is_empty());
}

// ---------------------------------------------------------------------------
// Combinação de múltiplos subscribers no mesmo router (builder programático).
// ---------------------------------------------------------------------------

#[test]
fn multiplos_subscribers_compoem_no_mesmo_router() {
    let router = handle_order_ack::register(EventRouter::new());
    let router = handle_order_with_publish::register(router);
    let router = handle_silent::register(router);

    assert_eq!(
        router.subscription_topics(),
        vec![
            "orders.created".to_string(),
            "orders.created".to_string(),
            "orders.silent".to_string(),
        ]
    );
}
