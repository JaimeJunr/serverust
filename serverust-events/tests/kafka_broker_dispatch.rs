//! Testes para `KafkaBroker::dispatch` (US-7) — verifica o despacho
//! mockado para handlers inscritos sem exigir conexão real ao broker.
//!
//! O loop real `run_consumer_loop` é apenas o glue entre `recv()` do
//! rdkafka e este método; testá-lo exigiria broker físico.

#![cfg(feature = "kafka")]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use serverust_events::broker::Broker;
use serverust_events::broker::BrokerMessage;
use serverust_events::broker::kafka::{KafkaBroker, KafkaBrokerConfig};

fn make_broker() -> KafkaBroker {
    KafkaBroker::with_config(KafkaBrokerConfig {
        brokers: "localhost:9092".to_string(),
        region: "us-east-1".to_string(),
        iam_auth: false,
    })
    .expect("KafkaBroker init com bootstrap fake deve funcionar (sem conectar)")
}

#[tokio::test]
async fn dispatch_invoca_handlers_inscritos_no_topico() {
    let broker = make_broker();
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));

    let received_clone = received.clone();
    broker
        .subscribe(
            "orders.created",
            Arc::new(move |msg: BrokerMessage| {
                let received = received_clone.clone();
                Box::pin(async move {
                    received.lock().unwrap().push(msg.payload);
                    Ok(())
                })
            }),
        )
        .await
        .unwrap();

    let msg = BrokerMessage {
        topic: "orders.created".to_string(),
        partition: Some(0),
        offset: Some(7),
        key: Some(b"order-7".to_vec()),
        payload: b"hello".to_vec(),
        headers: HashMap::new(),
        timestamp: None,
    };
    broker.dispatch(msg).await.unwrap();

    let got = received.lock().unwrap().clone();
    assert_eq!(got, vec![b"hello".to_vec()]);
}

#[tokio::test]
async fn dispatch_em_topico_sem_subscriber_e_no_op() {
    let broker = make_broker();
    let msg = BrokerMessage {
        topic: "topico.sem.subscriber".to_string(),
        partition: None,
        offset: None,
        key: None,
        payload: Vec::new(),
        headers: HashMap::new(),
        timestamp: None,
    };
    broker.dispatch(msg).await.unwrap();
}

#[tokio::test]
async fn subscribed_topics_lista_topicos_inscritos() {
    let broker = make_broker();
    let h = |_: BrokerMessage| -> serverust_events::broker::HandlerFuture {
        Box::pin(async { Ok(()) })
    };
    broker.subscribe("topic.a", Arc::new(h)).await.unwrap();
    broker.subscribe("topic.b", Arc::new(h)).await.unwrap();
    broker.subscribe("topic.a", Arc::new(h)).await.unwrap();

    let mut topics = broker.subscribed_topics();
    topics.sort();
    topics.dedup();
    assert_eq!(topics, vec!["topic.a".to_string(), "topic.b".to_string()]);
}
