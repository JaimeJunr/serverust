//! Testes para a trait `Broker` e tipos públicos relacionados (US-1).
//!
//! Não exigem broker físico — exercitam um mock in-test que prova a
//! ergonomia da trait, e o construtor do `KafkaBroker` quando a feature
//! `kafka` está ativa (sem chamadas de rede).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use serverust_events::broker::{Broker, BrokerError, BrokerMessage, HandlerFuture};

#[derive(Default)]
struct RecordingBroker {
    published: Mutex<Vec<(String, Vec<u8>)>>,
    subscriptions: Mutex<Vec<String>>,
}

#[async_trait]
impl Broker for RecordingBroker {
    async fn subscribe(
        &self,
        topic: &str,
        _handler: Arc<dyn Fn(BrokerMessage) -> HandlerFuture + Send + Sync>,
    ) -> Result<(), BrokerError> {
        self.subscriptions.lock().unwrap().push(topic.to_string());
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), BrokerError> {
        self.published
            .lock()
            .unwrap()
            .push((topic.to_string(), payload.to_vec()));
        Ok(())
    }
}

fn noop_handler() -> Arc<dyn Fn(BrokerMessage) -> HandlerFuture + Send + Sync> {
    Arc::new(|_msg: BrokerMessage| -> HandlerFuture {
        let fut: Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>> =
            Box::pin(async { Ok(()) });
        fut
    })
}

#[tokio::test]
async fn broker_pode_ser_usado_como_trait_object() {
    let broker: Arc<dyn Broker> = Arc::new(RecordingBroker::default());
    broker
        .subscribe("orders.created", noop_handler())
        .await
        .unwrap();
    broker.publish("orders.confirmed", b"hello").await.unwrap();
}

#[tokio::test]
async fn publish_registra_payload_e_topico() {
    let broker = RecordingBroker::default();
    broker.publish("topic-a", b"payload-1").await.unwrap();
    broker.publish("topic-b", b"payload-2").await.unwrap();

    let published = broker.published.lock().unwrap().clone();
    assert_eq!(
        published,
        vec![
            ("topic-a".to_string(), b"payload-1".to_vec()),
            ("topic-b".to_string(), b"payload-2".to_vec()),
        ]
    );
}

#[tokio::test]
async fn subscribe_registra_topico_inscrito() {
    let broker = RecordingBroker::default();
    broker
        .subscribe("orders.created", noop_handler())
        .await
        .unwrap();
    broker
        .subscribe("orders.cancelled", noop_handler())
        .await
        .unwrap();

    let subs = broker.subscriptions.lock().unwrap().clone();
    assert_eq!(subs, vec!["orders.created", "orders.cancelled"]);
}

#[test]
fn broker_message_expoe_campos_essenciais() {
    let mut headers = HashMap::new();
    headers.insert("correlation-id".to_string(), b"abc".to_vec());

    let msg = BrokerMessage {
        topic: "orders.created".to_string(),
        partition: Some(0),
        offset: Some(42),
        key: Some(b"order-1".to_vec()),
        payload: b"{}".to_vec(),
        headers,
        timestamp: None,
    };

    assert_eq!(msg.topic, "orders.created");
    assert_eq!(msg.partition, Some(0));
    assert_eq!(msg.offset, Some(42));
    assert_eq!(msg.key.as_deref(), Some(b"order-1".as_ref()));
    assert_eq!(msg.payload, b"{}");
    assert_eq!(
        msg.headers.get("correlation-id").map(Vec::as_slice),
        Some(b"abc".as_ref())
    );
}

#[test]
fn broker_error_implementa_traits_basicas() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BrokerError>();

    let err = BrokerError::Configuration("missing brokers".into());
    assert!(format!("{err}").contains("missing brokers"));
}

#[cfg(feature = "kafka")]
mod kafka_broker_tests {
    use super::*;
    use serverust_events::broker::kafka::KafkaBroker;

    #[test]
    fn kafka_broker_implementa_broker() {
        fn assert_broker<T: Broker>() {}
        assert_broker::<KafkaBroker>();
    }

    #[test]
    fn kafka_broker_falha_quando_brokers_ausentes() {
        // Salva valores anteriores para restaurar após o teste e evitar
        // interferência com outros testes que podem rodar em paralelo.
        let prev_msk = std::env::var("MSK_BOOTSTRAP_SERVERS").ok();
        let prev_kafka = std::env::var("KAFKA_BROKERS").ok();

        unsafe {
            std::env::remove_var("MSK_BOOTSTRAP_SERVERS");
            std::env::remove_var("KAFKA_BROKERS");
        }

        let result = KafkaBroker::from_env();

        unsafe {
            match prev_msk {
                Some(v) => std::env::set_var("MSK_BOOTSTRAP_SERVERS", v),
                None => std::env::remove_var("MSK_BOOTSTRAP_SERVERS"),
            }
            match prev_kafka {
                Some(v) => std::env::set_var("KAFKA_BROKERS", v),
                None => std::env::remove_var("KAFKA_BROKERS"),
            }
        }

        match result {
            Ok(_) => panic!("sem brokers, deve falhar"),
            Err(err) => assert!(matches!(err, BrokerError::Configuration(_))),
        }
    }
}
