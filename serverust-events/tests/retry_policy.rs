//! Testes de US-5: RetryPolicy in-memory — retry loop, backoff exponencial, DLQ.
//!
//! Todos os testes precisam de `InMemoryBroker` + API `attach(Arc<B>)`, então
//! ficam atrás de `cfg(feature = "in-memory")`.

#![cfg(feature = "in-memory")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serverust_events::broker::in_memory::InMemoryBroker;
use serverust_events::broker::{Broker, BrokerError};
use serverust_events::retry::RetryPolicy;
use serverust_events::router::EventRouter;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct OrderEvent {
    id: u64,
}

// ---------------------------------------------------------------------------
// Happy path: nenhum retry necessário — handler bem-sucedido na 1ª tentativa.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handler_bem_sucedido_sem_retry() {
    let received = Arc::new(AtomicU32::new(0));
    let sink = received.clone();

    let broker = Arc::new(InMemoryBroker::new());
    EventRouter::new()
        .subscribe::<OrderEvent, _, _>("orders", move |_: OrderEvent| {
            let s = sink.clone();
            async move {
                s.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .with_retry(RetryPolicy::immediate(3))
        .attach(broker.clone())
        .await
        .unwrap();

    let payload = serde_json::to_vec(&OrderEvent { id: 1 }).unwrap();
    broker.publish("orders", &payload).await.unwrap();

    // Handler chamado apenas 1 vez — sem retry desnecessário.
    assert_eq!(received.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// Retry path: handler falha nas primeiras N-1 tentativas, sucede na N-ésima.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handler_retenta_ate_sucesso_com_immediate_policy() {
    let call_count = Arc::new(AtomicU32::new(0));
    let counter = call_count.clone();

    let broker = Arc::new(InMemoryBroker::new());
    EventRouter::new()
        .subscribe::<OrderEvent, _, _>("orders", move |_: OrderEvent| {
            let c = counter.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    // Falha nas tentativas 0 e 1.
                    Err(BrokerError::Subscribe("falha simulada".to_string()))
                } else {
                    Ok(())
                }
            }
        })
        .with_retry(RetryPolicy::immediate(3))
        .attach(broker.clone())
        .await
        .unwrap();

    let payload = serde_json::to_vec(&OrderEvent { id: 2 }).unwrap();
    broker.publish("orders", &payload).await.unwrap();

    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

// ---------------------------------------------------------------------------
// Esgotamento: todas as tentativas falham — erro propagado, sem DLQ.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn todas_as_tentativas_esgotam_sem_dlq_retorna_erro() {
    let call_count = Arc::new(AtomicU32::new(0));
    let counter = call_count.clone();

    let broker = Arc::new(InMemoryBroker::new());
    EventRouter::new()
        .subscribe::<OrderEvent, _, _>("orders", move |_: OrderEvent| {
            let c = counter.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(BrokerError::Subscribe("sempre falha".to_string()))
            }
        })
        .with_retry(RetryPolicy::immediate(2))
        .attach(broker.clone())
        .await
        .unwrap();

    let payload = serde_json::to_vec(&OrderEvent { id: 3 }).unwrap();
    let result = broker.publish("orders", &payload).await;

    assert!(result.is_err());
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
    assert_eq!(broker.messages("orders.dlq").len(), 0);
}

// ---------------------------------------------------------------------------
// DLQ via `dead_letter` em RetryPolicy: mensagem publicada no tópico DLQ.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dead_letter_publica_no_dlq_apos_esgotamento_via_policy() {
    let broker = Arc::new(InMemoryBroker::new());
    let payload = serde_json::to_vec(&OrderEvent { id: 4 }).unwrap();

    EventRouter::new()
        .subscribe::<OrderEvent, _, _>("orders", |_: OrderEvent| async move {
            Err(BrokerError::Subscribe("falha".to_string()))
        })
        .with_retry(RetryPolicy::immediate(2).dead_letter("orders.dlq"))
        .attach(broker.clone())
        .await
        .unwrap();

    let _ = broker.publish("orders", &payload).await;

    let dlq_msgs = broker.messages("orders.dlq");
    assert_eq!(dlq_msgs.len(), 1);
    assert_eq!(dlq_msgs[0].payload, payload);
}

// ---------------------------------------------------------------------------
// DLQ via `with_dlq` no EventRouter: mesma semântica — publicado no DLQ.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn with_dlq_publica_no_dlq_apos_esgotamento_via_router() {
    let broker = Arc::new(InMemoryBroker::new());
    let payload = serde_json::to_vec(&OrderEvent { id: 5 }).unwrap();

    EventRouter::new()
        .subscribe::<OrderEvent, _, _>("orders", |_: OrderEvent| async move {
            Err(BrokerError::Subscribe("falha".to_string()))
        })
        .with_retry(RetryPolicy::immediate(2))
        .with_dlq("orders.dlq")
        .attach(broker.clone())
        .await
        .unwrap();

    let _ = broker.publish("orders", &payload).await;

    let dlq_msgs = broker.messages("orders.dlq");
    assert_eq!(dlq_msgs.len(), 1);
    assert_eq!(dlq_msgs[0].payload, payload);
}

// ---------------------------------------------------------------------------
// Exponencial: handler retenta com backoff (base delay mínimo para testes).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn exponential_policy_retenta_com_backoff() {
    let call_count = Arc::new(AtomicU32::new(0));
    let counter = call_count.clone();

    let broker = Arc::new(InMemoryBroker::new());
    EventRouter::new()
        .subscribe::<OrderEvent, _, _>("orders", move |_: OrderEvent| {
            let c = counter.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(BrokerError::Subscribe("falha".to_string()))
                } else {
                    Ok(())
                }
            }
        })
        // Base delay de 1ms para manter o teste rápido.
        .with_retry(RetryPolicy::exponential(3, Duration::from_millis(1)))
        .attach(broker.clone())
        .await
        .unwrap();

    let payload = serde_json::to_vec(&OrderEvent { id: 6 }).unwrap();
    broker.publish("orders", &payload).await.unwrap();

    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

// ---------------------------------------------------------------------------
// DLQ exponencial: esgota backoff e publica no DLQ.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn exponential_dead_letter_publica_no_dlq() {
    let broker = Arc::new(InMemoryBroker::new());
    let payload = serde_json::to_vec(&OrderEvent { id: 7 }).unwrap();

    EventRouter::new()
        .subscribe::<OrderEvent, _, _>("orders", |_: OrderEvent| async move {
            Err(BrokerError::Subscribe("falha".to_string()))
        })
        .with_retry(RetryPolicy::exponential(2, Duration::from_millis(1)).dead_letter("orders.dlq"))
        .attach(broker.clone())
        .await
        .unwrap();

    let _ = broker.publish("orders", &payload).await;

    let dlq_msgs = broker.messages("orders.dlq");
    assert_eq!(dlq_msgs.len(), 1);
}
