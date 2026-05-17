//! Testes para `LambdaBroker` (US-7) — broker que despacha registros de
//! um `aws_lambda_events::KafkaEvent` para handlers inscritos.
//!
//! `LambdaBroker` não depende de `rdkafka`: está disponível mesmo sem a
//! feature `kafka`, porque em Lambda o transporte é resolvido pelo
//! runtime AWS e os bytes chegam já decodificados no event source.

use std::sync::Arc;
use std::sync::Mutex;

use aws_lambda_events::event::kafka::KafkaEvent;
use serde::Deserialize;
use serverust_events::broker::Broker;
use serverust_events::broker::lambda::LambdaBroker;
use serverust_events::router::EventRouter;

fn fixture() -> KafkaEvent {
    let raw = include_str!("fixtures/kafka/msk-v1.json");
    serde_json::from_str(raw).expect("fixture msk-v1.json deve ser KafkaEvent válido")
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
struct WalletCredit {
    amount: u64,
    currency: String,
}

#[tokio::test]
async fn handle_kafka_event_despacha_registros_para_handlers_inscritos() {
    let broker = Arc::new(LambdaBroker::new());
    let received: Arc<Mutex<Vec<WalletCredit>>> = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let received = received.clone();
        EventRouter::new().subscribe::<WalletCredit, _, _>("wallet.credits", move |event| {
            let received = received.clone();
            async move {
                received.lock().unwrap().push(event);
                Ok(())
            }
        })
    };

    router.attach(broker.clone()).await.unwrap();

    let event = fixture();
    broker.handle_kafka_event(&event).await.unwrap();

    let got = received.lock().unwrap().clone();
    assert_eq!(
        got,
        vec![
            WalletCredit {
                amount: 100,
                currency: "USD".into()
            },
            WalletCredit {
                amount: 250,
                currency: "EUR".into()
            },
        ]
    );
}

#[tokio::test]
async fn handle_kafka_event_ignora_topico_sem_subscriber() {
    let broker = Arc::new(LambdaBroker::new());
    // Nenhum subscriber registrado — não deve panicar nem erroar.
    let event = fixture();
    broker.handle_kafka_event(&event).await.unwrap();
}

#[tokio::test]
async fn handle_kafka_event_propaga_erro_de_decodificacao_base64() {
    let broker = Arc::new(LambdaBroker::new());
    let router =
        EventRouter::new().subscribe::<WalletCredit, _, _>("wallet.credits", |_| async { Ok(()) });
    router.attach(broker.clone()).await.unwrap();

    let mut event = fixture();
    let records = event.records.get_mut("wallet.credits-0").unwrap();
    records[0].value = Some("not-base64!!!".to_string());

    let err = broker.handle_kafka_event(&event).await.unwrap_err();
    assert!(format!("{err}").contains("base64"), "erro foi: {err}");
}

#[tokio::test]
async fn lambda_broker_implementa_broker_trait_object() {
    let broker: Arc<dyn Broker> = Arc::new(LambdaBroker::new());
    let router = EventRouter::new().subscribe::<WalletCredit, _, _>("x", |_| async { Ok(()) });
    router.attach(broker).await.unwrap();
}

#[tokio::test]
async fn lambda_broker_publish_falha_indicando_sink_only() {
    let broker = LambdaBroker::new();
    let err = broker.publish("any.topic", b"payload").await.unwrap_err();
    assert!(format!("{err}").contains("LambdaBroker"));
}
