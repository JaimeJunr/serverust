//! Transport abstraction (US-011) — paridade entre drivers Kafka e SQS.
//!
//! Demonstra os critérios de aceite de US-011:
//!
//! 1. Mesma função de negócio (`process_order`) é referenciada por ambas as
//!    macros `#[subscriber(...)]` — uma com `driver = "kafka"`, outra com
//!    `driver = "sqs"`. Se a abstração falhar em algum lado, o teste não
//!    compila.
//! 2. `EventRouter` aceita brokers heterogêneos no mesmo app: um teste cria
//!    `LambdaBroker` (kafka) e `SqsBroker` (sqs) simultaneamente, atacha cada
//!    subscriber ao seu broker, e ambos respondem aos seus event sources sem
//!    interferência.
//! 3. Paridade Ok/Err: o mesmo payload `Order` produz sucesso/erro consistente
//!    em ambos os caminhos. SQS reporta erro via `batchItemFailures`; Kafka
//!    propaga via `Err` no `handle_kafka_event` — semântica nativa de cada
//!    driver, mas o gatilho lógico (`amount == 0`) é idêntico.

#![cfg(feature = "sqs")]

use std::sync::Arc;
use std::sync::Mutex;

use aws_lambda_events::event::kafka::KafkaEvent;
use aws_lambda_events::event::sqs::SqsEvent;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serverust_events::broker::lambda::LambdaBroker;
use serverust_events::broker::{Broker, BrokerError};
use serverust_events::extract::State;
use serverust_events::router::EventRouter;
use serverust_events::sqs::consumer::SqsBroker;
use serverust_macros::subscriber;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Order {
    #[serde(rename = "orderId")]
    order_id: String,
    amount: u64,
}

/// Estado compartilhado: handle Arc-clonável que ambos os subscribers usam
/// para coletar as ordens processadas. Implementa `Clone` para que o teste
/// e o `EventRouter::with_state(...)` possam manter cópias independentes do
/// mesmo backing `Mutex<Vec<Order>>`.
#[derive(Clone)]
struct AppState {
    sink: Arc<Mutex<Vec<Order>>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            sink: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn snapshot(&self) -> Vec<Order> {
        self.sink.lock().expect("sink poisoned").clone()
    }
}

/// Lógica de negócio única — chamada pelos dois subscribers. `amount == 0`
/// dispara `Err` (gatilho de paridade de erro).
async fn process_order(order: Order, state: &AppState) -> Result<(), BrokerError> {
    if order.amount == 0 {
        return Err(BrokerError::Subscribe(format!(
            "zero amount in order {}",
            order.order_id
        )));
    }
    state.sink.lock().expect("sink poisoned").push(order);
    Ok(())
}

#[subscriber(driver = "kafka", topic = "orders")]
async fn kafka_orders(event: Order, state: State<AppState>) -> Result<(), BrokerError> {
    process_order(event, &state).await
}

#[subscriber(driver = "sqs", queue = "orders")]
async fn sqs_orders(event: Order, state: State<AppState>) -> Result<(), BrokerError> {
    process_order(event, &state).await
}

// ---------------------------------------------------------------------------
// Builders de eventos sintéticos
// ---------------------------------------------------------------------------

fn make_kafka_event(orders: &[Order]) -> KafkaEvent {
    let records: Vec<serde_json::Value> = orders
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let body = serde_json::to_string(o).expect("serialize Order");
            let value_b64 = base64::engine::general_purpose::STANDARD.encode(body.as_bytes());
            serde_json::json!({
                "topic": "orders",
                "partition": 0,
                "offset": 100 + i,
                "timestamp": 1_678_000_000_000_i64,
                "timestampType": "CREATE_TIME",
                "key": null,
                "value": value_b64,
                "headers": []
            })
        })
        .collect();
    let value = serde_json::json!({
        "eventSource": "aws:kafka",
        "eventSourceArn": "arn:aws:kafka:us-east-1:123456789012:cluster/MyCluster/abc",
        "bootstrapServers": "broker.kafka.local:9092",
        "records": {
            "orders-0": records,
        },
    });
    serde_json::from_value(value).expect("synthetic KafkaEvent must parse")
}

fn make_sqs_event(orders: &[Order]) -> SqsEvent {
    let records: Vec<serde_json::Value> = orders
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let body = serde_json::to_string(o).expect("serialize Order");
            serde_json::json!({
                "messageId": format!("msg-{i}"),
                "receiptHandle": "AQEB...",
                "body": body,
                "md5OfBody": "ignored",
                "attributes": {
                    "ApproximateReceiveCount": "1"
                },
                "messageAttributes": {},
                "eventSourceARN": "arn:aws:sqs:us-east-1:123456789012:orders",
                "eventSource": "aws:sqs",
                "awsRegion": "us-east-1"
            })
        })
        .collect();
    serde_json::from_value(serde_json::json!({ "Records": records }))
        .expect("synthetic SqsEvent must parse")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Critério #1: cada macro emite o driver correto para a mesma chave lógica
/// `orders`. Compila se e somente se a mesma assinatura de handler é aceita
/// pelos dois drivers.
#[test]
fn macros_compilam_para_ambos_drivers_com_a_mesma_assinatura() {
    assert_eq!(kafka_orders::DRIVER, "kafka");
    assert_eq!(kafka_orders::SUBSCRIBE_TOPIC, "orders");
    assert_eq!(sqs_orders::DRIVER, "sqs");
    assert_eq!(sqs_orders::SUBSCRIBE_TOPIC, "orders");
}

/// Critério #2: brokers heterogêneos no mesmo app.
///
/// `LambdaBroker` e `SqsBroker` coexistem; cada subscriber é atachado ao seu
/// broker via `Arc<dyn Broker>`. Ambos compartilham o mesmo `AppState`,
/// provando que a abstração de transporte não força isolamento de estado.
#[tokio::test]
async fn brokers_heterogeneos_no_mesmo_app() {
    let state = AppState::new();

    let kafka_broker = Arc::new(LambdaBroker::new());
    let sqs_broker = Arc::new(SqsBroker::new());

    kafka_orders::register(EventRouter::new().with_state(state.clone()))
        .attach(kafka_broker.clone() as Arc<dyn Broker>)
        .await
        .expect("attach kafka subscriber");
    sqs_orders::register(EventRouter::new().with_state(state.clone()))
        .attach(sqs_broker.clone() as Arc<dyn Broker>)
        .await
        .expect("attach sqs subscriber");

    // Kafka event source.
    kafka_broker
        .handle_kafka_event(&make_kafka_event(&[Order {
            order_id: "k-1".into(),
            amount: 10,
        }]))
        .await
        .expect("kafka dispatch");

    // SQS event source.
    let sqs_resp = sqs_broker
        .handle_sqs_event(&make_sqs_event(&[Order {
            order_id: "s-1".into(),
            amount: 20,
        }]))
        .await;
    assert!(
        sqs_resp.batch_item_failures.is_empty(),
        "no failures expected; got {:?}",
        sqs_resp.batch_item_failures
    );

    assert_eq!(
        state.snapshot(),
        vec![
            Order {
                order_id: "k-1".into(),
                amount: 10
            },
            Order {
                order_id: "s-1".into(),
                amount: 20
            },
        ]
    );
}

/// Critério #3a: paridade Ok — payload válido produz sucesso em ambos.
#[tokio::test]
async fn ok_parity_entre_kafka_e_sqs() {
    let order = Order {
        order_id: "ok-1".into(),
        amount: 42,
    };

    // Kafka side.
    let kafka_state = AppState::new();
    let kafka_broker = Arc::new(LambdaBroker::new());
    kafka_orders::register(EventRouter::new().with_state(kafka_state.clone()))
        .attach(kafka_broker.clone())
        .await
        .unwrap();
    kafka_broker
        .handle_kafka_event(&make_kafka_event(std::slice::from_ref(&order)))
        .await
        .expect("kafka must succeed");
    assert_eq!(kafka_state.snapshot(), vec![order.clone()]);

    // SQS side.
    let sqs_state = AppState::new();
    let sqs_broker = Arc::new(SqsBroker::new());
    sqs_orders::register(EventRouter::new().with_state(sqs_state.clone()))
        .attach(sqs_broker.clone())
        .await
        .unwrap();
    let sqs_resp = sqs_broker
        .handle_sqs_event(&make_sqs_event(std::slice::from_ref(&order)))
        .await;
    assert!(
        sqs_resp.batch_item_failures.is_empty(),
        "sqs path: {:?}",
        sqs_resp.batch_item_failures
    );
    assert_eq!(sqs_state.snapshot(), vec![order]);
}

/// Critério #3b: paridade Err — `amount == 0` aciona `Err` em ambos os
/// caminhos. Cada driver reporta usando sua semântica nativa:
///
/// - Kafka (`LambdaBroker::handle_kafka_event`): propaga `BrokerError` em
///   `Err(...)` para fora.
/// - SQS (`SqsBroker::handle_sqs_event`): nunca retorna `Err`; reporta a
///   mensagem que falhou em `batchItemFailures` (modelo Lambda ESM).
///
/// O teste valida que a MESMA causa lógica de erro é capturada nos dois
/// canais — o que importa para US-011.
#[tokio::test]
async fn err_parity_entre_kafka_e_sqs() {
    let bad = Order {
        order_id: "bad-1".into(),
        amount: 0,
    };

    // Kafka side.
    let kafka_state = AppState::new();
    let kafka_broker = Arc::new(LambdaBroker::new());
    kafka_orders::register(EventRouter::new().with_state(kafka_state.clone()))
        .attach(kafka_broker.clone())
        .await
        .unwrap();
    let kafka_err = kafka_broker
        .handle_kafka_event(&make_kafka_event(std::slice::from_ref(&bad)))
        .await
        .expect_err("kafka must propagate handler error");
    let kafka_err_str = format!("{kafka_err}");
    assert!(
        kafka_err_str.contains("zero amount"),
        "kafka error should preserve cause; got {kafka_err_str}"
    );
    assert!(kafka_state.snapshot().is_empty(), "no order persisted");

    // SQS side.
    let sqs_state = AppState::new();
    let sqs_broker = Arc::new(SqsBroker::new());
    sqs_orders::register(EventRouter::new().with_state(sqs_state.clone()))
        .attach(sqs_broker.clone())
        .await
        .unwrap();
    let sqs_resp = sqs_broker.handle_sqs_event(&make_sqs_event(&[bad])).await;
    let failures: Vec<String> = sqs_resp
        .batch_item_failures
        .iter()
        .map(|f| f.item_identifier.clone())
        .collect();
    assert_eq!(
        failures,
        vec!["msg-0".to_string()],
        "sqs must report the failing messageId"
    );
    assert!(sqs_state.snapshot().is_empty(), "no order persisted");
}
