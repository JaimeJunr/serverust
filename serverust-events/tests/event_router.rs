//! Testes do `EventRouter` e do builder programático (US-3).
//!
//! Cobre:
//! - construção via `EventRouter::new()` + `subscribe::<T, _>(topic, handler)`
//! - `with_retry(RetryPolicy::exponential(...))` aplicado à última inscrição
//! - `with_dlq("topic")` aplicado à última inscrição
//! - injeção de qualquer `impl Broker` (com e sem `in-memory`)
//!
//! O subconjunto que precisa de `InMemoryBroker` para entrega real fica
//! atrás de `cfg(feature = "in-memory")`. Sem a feature, `cargo test
//! -p serverust-events` continua verde porque os testes restantes só
//! exercitam compilação do builder e configuração das inscrições.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serverust_events::broker::{Broker, BrokerError, BrokerMessage, HandlerFuture};
use serverust_events::retry::RetryPolicy;
use serverust_events::router::EventRouter;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct OrderCreated {
    id: u64,
    total: u32,
}

async fn handle_order(_event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Broker dummy: usado para garantir que `attach` aceita qualquer `impl Broker`
// sem precisar da feature `in-memory`.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct DummyBroker {
    subscribed: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl Broker for DummyBroker {
    async fn subscribe(
        &self,
        topic: &str,
        _handler: Arc<dyn Fn(BrokerMessage) -> HandlerFuture + Send + Sync>,
    ) -> Result<(), BrokerError> {
        self.subscribed.lock().unwrap().push(topic.to_string());
        Ok(())
    }

    async fn publish(&self, _topic: &str, _payload: &[u8]) -> Result<(), BrokerError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AC: builder compila e aceita handler tipado
// ---------------------------------------------------------------------------

#[test]
fn event_router_new_e_subscribe_compilam_com_handler_tipado() {
    let router = EventRouter::new().subscribe::<OrderCreated, _, _>("orders.created", handle_order);
    assert_eq!(
        router.subscription_topics(),
        vec!["orders.created".to_string()]
    );
}

#[test]
fn event_router_aceita_multiplas_inscricoes_encadeadas() {
    let router = EventRouter::new()
        .subscribe::<OrderCreated, _, _>("orders.created", handle_order)
        .subscribe::<OrderCreated, _, _>("orders.updated", handle_order);
    assert_eq!(
        router.subscription_topics(),
        vec!["orders.created".to_string(), "orders.updated".to_string()]
    );
}

#[test]
fn event_router_subscribe_aceita_closure_async() {
    let router = EventRouter::new()
        .subscribe::<OrderCreated, _, _>("orders.created", |_event: OrderCreated| async move {
            Ok::<_, BrokerError>(())
        });
    assert_eq!(router.subscription_topics().len(), 1);
}

// ---------------------------------------------------------------------------
// AC: with_retry aplica RetryPolicy à última inscrição
// ---------------------------------------------------------------------------

#[test]
fn with_retry_aplica_policy_a_ultima_inscricao() {
    let router = EventRouter::new()
        .subscribe::<OrderCreated, _, _>("orders.created", handle_order)
        .with_retry(RetryPolicy::exponential(3, Duration::from_secs(1)));

    let retry = router.last_retry().expect("retry deve estar configurado");
    match retry {
        RetryPolicy::Exponential {
            max_attempts,
            base_delay,
        } => {
            assert_eq!(*max_attempts, 3);
            assert_eq!(*base_delay, Duration::from_secs(1));
        }
        other => panic!("esperava Exponential, recebeu {other:?}"),
    }
}

#[test]
fn retry_policy_immediate_construtor_funciona() {
    let policy = RetryPolicy::immediate(5);
    match policy {
        RetryPolicy::Immediate { max_attempts } => assert_eq!(max_attempts, 5),
        other => panic!("esperava Immediate, recebeu {other:?}"),
    }
}

#[test]
fn with_retry_sem_subscribe_e_no_op() {
    let router = EventRouter::new().with_retry(RetryPolicy::immediate(3));
    assert!(router.subscription_topics().is_empty());
}

// ---------------------------------------------------------------------------
// AC: with_dlq configura dead letter queue
// ---------------------------------------------------------------------------

#[test]
fn with_dlq_aplica_topic_a_ultima_inscricao() {
    let router = EventRouter::new()
        .subscribe::<OrderCreated, _, _>("orders.created", handle_order)
        .with_dlq("orders.dlq");

    assert_eq!(router.last_dlq(), Some("orders.dlq"));
}

#[test]
fn with_dlq_sem_subscribe_e_no_op() {
    let router = EventRouter::new().with_dlq("orders.dlq");
    assert_eq!(router.last_dlq(), None);
}

#[test]
fn with_retry_e_with_dlq_compostos_em_pipeline_fluente() {
    let router = EventRouter::new()
        .subscribe::<OrderCreated, _, _>("orders.created", handle_order)
        .with_retry(RetryPolicy::exponential(3, Duration::from_secs(1)))
        .with_dlq("orders.dlq");

    assert!(router.last_retry().is_some());
    assert_eq!(router.last_dlq(), Some("orders.dlq"));
}

// ---------------------------------------------------------------------------
// AC: EventRouter aceita qualquer `impl Broker` injetado
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attach_aceita_qualquer_impl_broker() {
    let broker = DummyBroker::default();
    let router = EventRouter::new()
        .subscribe::<OrderCreated, _, _>("orders.created", handle_order)
        .subscribe::<OrderCreated, _, _>("orders.updated", handle_order);

    router.attach(&broker).await.unwrap();

    let subscribed = broker.subscribed.lock().unwrap().clone();
    assert_eq!(subscribed, vec!["orders.created", "orders.updated"]);
}

#[tokio::test]
async fn attach_aceita_broker_via_trait_object() {
    let broker: Arc<dyn Broker> = Arc::new(DummyBroker::default());
    let router = EventRouter::new().subscribe::<OrderCreated, _, _>("orders.created", handle_order);

    router.attach(broker.as_ref()).await.unwrap();
}

// ---------------------------------------------------------------------------
// Integração: handler tipado recebe payload JSON decodificado
// ---------------------------------------------------------------------------

#[cfg(feature = "in-memory")]
mod with_in_memory {
    use super::*;
    use serverust_events::broker::in_memory::InMemoryBroker;
    use std::sync::Mutex;

    #[tokio::test]
    async fn handler_recebe_payload_decodificado_via_in_memory_broker() {
        let received: Arc<Mutex<Vec<OrderCreated>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();

        let broker = InMemoryBroker::new();
        EventRouter::new()
            .subscribe::<OrderCreated, _, _>("orders.created", move |event: OrderCreated| {
                let sink = sink.clone();
                async move {
                    sink.lock().unwrap().push(event);
                    Ok(())
                }
            })
            .attach(&broker)
            .await
            .unwrap();

        let payload = serde_json::to_vec(&OrderCreated { id: 7, total: 42 }).unwrap();
        broker.publish("orders.created", &payload).await.unwrap();

        let got = received.lock().unwrap().clone();
        assert_eq!(got, vec![OrderCreated { id: 7, total: 42 }]);
    }

    #[tokio::test]
    async fn payload_invalido_propaga_broker_error_subscribe() {
        let broker = InMemoryBroker::new();
        EventRouter::new()
            .subscribe::<OrderCreated, _, _>("orders.created", handle_order)
            .attach(&broker)
            .await
            .unwrap();

        let result = broker.publish("orders.created", b"not-json").await;
        assert!(matches!(result, Err(BrokerError::Subscribe(_))));
    }
}
