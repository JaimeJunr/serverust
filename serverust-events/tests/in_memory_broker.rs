//! Testes do `InMemoryBroker` (US-2).
//!
//! Exercitam publicação, inscrição, entrega de mensagens em memória e
//! acesso ao histórico via `broker.messages("topic")`.
//!
//! O arquivo inteiro é protegido por `cfg(feature = "in-memory")`: sem a
//! flag o binário de teste é vazio e passa trivialmente em `cargo test -p
//! serverust-events`.

#![cfg(feature = "in-memory")]

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serverust_events::broker::in_memory::InMemoryBroker;
use serverust_events::broker::{BoxedHandler, Broker, BrokerError, BrokerMessage};

fn capturing_handler(sink: Arc<Mutex<Vec<Vec<u8>>>>) -> BoxedHandler {
    Arc::new(
        move |msg: BrokerMessage| -> Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>> {
            let sink = sink.clone();
            Box::pin(async move {
                sink.lock().unwrap().push(msg.payload);
                Ok(())
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Implementa a trait corretamente
// ---------------------------------------------------------------------------

#[test]
fn in_memory_broker_implementa_broker_send_sync() {
    fn assert_broker<T: Broker + Send + Sync>() {}
    assert_broker::<InMemoryBroker>();
}

#[tokio::test]
async fn in_memory_broker_pode_ser_usado_como_trait_object() {
    let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
    broker.publish("test", b"hello").await.unwrap();
}

// ---------------------------------------------------------------------------
// messages() expõe histórico de publicações
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mensagens_publicadas_ficam_disponiveis_via_messages() {
    let broker = InMemoryBroker::new();
    broker.publish("orders", b"payload-1").await.unwrap();
    broker.publish("orders", b"payload-2").await.unwrap();

    let msgs = broker.messages("orders");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].payload, b"payload-1");
    assert_eq!(msgs[1].payload, b"payload-2");
}

#[tokio::test]
async fn messages_retorna_vazio_para_topico_desconhecido() {
    let broker = InMemoryBroker::new();
    assert!(broker.messages("nonexistent").is_empty());
}

#[tokio::test]
async fn messages_de_topicos_distintos_nao_se_misturam() {
    let broker = InMemoryBroker::new();
    broker.publish("a", b"msg-a").await.unwrap();
    broker.publish("b", b"msg-b").await.unwrap();

    assert_eq!(broker.messages("a").len(), 1);
    assert_eq!(broker.messages("b").len(), 1);
    assert_eq!(broker.messages("a")[0].payload, b"msg-a");
    assert_eq!(broker.messages("b")[0].payload, b"msg-b");
}

// ---------------------------------------------------------------------------
// Subscribers recebem mensagens publicadas
// ---------------------------------------------------------------------------

#[tokio::test]
async fn subscriber_recebe_mensagem_publicada() {
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(vec![]));
    let broker = InMemoryBroker::new();

    broker
        .subscribe("orders", capturing_handler(received.clone()))
        .await
        .unwrap();
    broker.publish("orders", b"hello").await.unwrap();

    let got = received.lock().unwrap().clone();
    assert_eq!(got, vec![b"hello".to_vec()]);
}

#[tokio::test]
async fn multiplos_subscribers_recebem_a_mesma_mensagem() {
    let count = Arc::new(Mutex::new(0u32));
    let broker = InMemoryBroker::new();

    for _ in 0..3 {
        let c = count.clone();
        let handler: BoxedHandler = Arc::new(move |_msg: BrokerMessage| {
            let c = c.clone();
            Box::pin(async move {
                *c.lock().unwrap() += 1;
                Ok(())
            }) as Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>>
        });
        broker.subscribe("events", handler).await.unwrap();
    }

    broker.publish("events", b"data").await.unwrap();
    assert_eq!(*count.lock().unwrap(), 3);
}

#[tokio::test]
async fn publish_em_topico_diferente_nao_aciona_subscriber() {
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(vec![]));
    let broker = InMemoryBroker::new();

    broker
        .subscribe("orders", capturing_handler(received.clone()))
        .await
        .unwrap();
    broker.publish("payments", b"other").await.unwrap();

    assert!(received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn subscriber_recebe_multiplas_publicacoes_sequenciais() {
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(vec![]));
    let broker = InMemoryBroker::new();

    broker
        .subscribe("stream", capturing_handler(received.clone()))
        .await
        .unwrap();
    broker.publish("stream", b"first").await.unwrap();
    broker.publish("stream", b"second").await.unwrap();
    broker.publish("stream", b"third").await.unwrap();

    let got = received.lock().unwrap().clone();
    assert_eq!(
        got,
        vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()]
    );
}
