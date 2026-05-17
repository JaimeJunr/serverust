//! Testes de extractors SQS (US-002):
//! - `Json<T>` desserializa o body da SqsMessage
//! - `State<S>` injeta estado compartilhado do Container DI
//! - `SqsMetadata` expõe message_id, receipt_handle, attributes, message_attributes
//! - Erro de desserialização retorna Err com motivo preservado
//! - Cada extractor testado em isolamento e em combinação

#![cfg(feature = "sqs")]

use std::sync::{Arc, Mutex};

use std::collections::HashMap;

use aws_lambda_events::event::sqs::SqsEvent;
use serde::{Deserialize, Serialize};
use serverust_events::{
    broker::{BrokerError, BrokerMessage},
    extract::{FromExtractor, Json, State},
    router::EventRouter,
    sqs::{consumer::SqsBroker, extract::SqsMetadata},
};

fn fixture() -> SqsEvent {
    let raw = include_str!("fixtures/sqs/standard.json");
    serde_json::from_str(raw).expect("fixture standard.json deve ser SqsEvent valido")
}

fn single_record_event(body: &str) -> SqsEvent {
    let raw = format!(
        r#"{{"Records":[{{"messageId":"test-id-1","receiptHandle":"rh-1","body":{body_json},"attributes":{{"ApproximateReceiveCount":"1","SentTimestamp":"1678000000000"}},"messageAttributes":{{"x_custom":{{"stringValue":"val-custom","dataType":"String"}}}},"eventSourceARN":"arn:aws:sqs:us-east-1:123456789012:orders","eventSource":"aws:sqs","awsRegion":"us-east-1"}}]}}"#,
        body_json = serde_json::to_string(body).unwrap()
    );
    serde_json::from_str(&raw).expect("single record event deve ser SqsEvent valido")
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
struct Order {
    #[serde(rename = "orderId")]
    order_id: String,
    amount: u64,
}

// ---------------------------------------------------------------------------
// AC: Json<T> desserializa o body da SqsMessage (happy path)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn json_extractor_deserializa_body_corretamente() {
    let broker = Arc::new(SqsBroker::new());
    let captured: Arc<Mutex<Vec<Order>>> = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let captured = captured.clone();
        EventRouter::new().subscribe_with("orders", move |Json(order): Json<Order>| {
            let captured = captured.clone();
            async move {
                captured.lock().unwrap().push(order);
                Ok(())
            }
        })
    };

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert!(
        resp.batch_item_failures.is_empty(),
        "nenhuma falha esperada, got: {:?}",
        resp.batch_item_failures
    );

    let got = captured.lock().unwrap().clone();
    assert_eq!(got.len(), 3);
    assert_eq!(
        got[0],
        Order {
            order_id: "order-1".into(),
            amount: 100
        }
    );
}

// ---------------------------------------------------------------------------
// AC: Erro de desserialização retorna Err com motivo preservado
// ---------------------------------------------------------------------------

#[tokio::test]
async fn json_extractor_erro_desserializacao_retorna_err_com_motivo() {
    let broker = Arc::new(SqsBroker::new());

    EventRouter::new()
        .subscribe_with("orders", |Json(_): Json<Order>| async { Ok(()) })
        .attach(broker.clone())
        .await
        .unwrap();

    // Injeta body inválido diretamente via evento sintético.
    let raw = r#"{"Records":[{"messageId":"bad-1","receiptHandle":"rh","body":"not-json-at-all","attributes":{},"messageAttributes":{},"eventSourceARN":"arn:aws:sqs:us-east-1:1:orders","eventSource":"aws:sqs","awsRegion":"us-east-1"}]}"#;
    let event: SqsEvent = serde_json::from_str(raw).unwrap();
    let resp = broker.handle_sqs_event(&event).await;

    assert_eq!(
        resp.batch_item_failures.len(),
        1,
        "mensagem com body inválido deve aparecer em batchItemFailures"
    );
    assert_eq!(resp.batch_item_failures[0].item_identifier, "bad-1");
}

// ---------------------------------------------------------------------------
// AC: SqsMetadata expõe message_id, receipt_handle, attributes, message_attributes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sqs_metadata_expoe_todos_os_campos_necessarios() {
    type MetaCaptured = Arc<Mutex<Vec<(String, String, String, String)>>>;
    let broker = Arc::new(SqsBroker::new());
    let captured: MetaCaptured = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let captured = captured.clone();
        EventRouter::new().subscribe_with(
            "orders",
            move |Json(order): Json<Order>, meta: SqsMetadata| {
                let captured = captured.clone();
                async move {
                    let approx_count = meta
                        .attributes
                        .get("ApproximateReceiveCount")
                        .cloned()
                        .unwrap_or_default();
                    let trace = meta
                        .message_attributes
                        .get("trace_id")
                        .and_then(|a| a.string_value.clone())
                        .unwrap_or_default();
                    captured.lock().unwrap().push((
                        meta.message_id.clone(),
                        meta.receipt_handle.clone(),
                        approx_count,
                        trace,
                    ));
                    drop(order);
                    Ok(())
                }
            },
        )
    };

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert!(resp.batch_item_failures.is_empty());

    let got = captured.lock().unwrap().clone();
    assert_eq!(got.len(), 3);
    // message_id
    assert_eq!(got[0].0, "11d6ee51-4cc7-4302-9e22-7cd8afdaadf5");
    // receipt_handle presente (não vazio)
    assert!(!got[0].1.is_empty());
    // attributes (system): ApproximateReceiveCount acessível
    assert_eq!(got[0].2, "1");
    // message_attributes (user): trace_id da primeira mensagem
    assert_eq!(got[0].3, "abc-123");
    // segunda mensagem sem trace_id → string vazia
    assert_eq!(got[1].3, "");
}

// ---------------------------------------------------------------------------
// AC: State<S> injeta estado compartilhado do DI Container (em contexto SQS)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn state_extractor_injeta_estado_em_handler_sqs() {
    let broker = Arc::new(SqsBroker::new());
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let captured = captured.clone();
        EventRouter::new()
            .with_state("prefixo::".to_string())
            .subscribe_with(
                "orders",
                move |Json(order): Json<Order>, state: State<String>| {
                    let captured = captured.clone();
                    async move {
                        captured
                            .lock()
                            .unwrap()
                            .push(format!("{}{}", *state, order.order_id));
                        Ok(())
                    }
                },
            )
    };

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert!(resp.batch_item_failures.is_empty());

    let got = captured.lock().unwrap().clone();
    assert_eq!(
        got,
        vec!["prefixo::order-1", "prefixo::order-2", "prefixo::order-3"]
    );
}

// ---------------------------------------------------------------------------
// AC: State ausente retorna Err descritivo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn state_extractor_ausente_retorna_err_descritivo() {
    let broker = Arc::new(SqsBroker::new());

    EventRouter::new()
        .subscribe_with("orders", |_: Json<Order>, _s: State<String>| async {
            Ok(())
        })
        .attach(broker.clone())
        .await
        .unwrap();

    let event = single_record_event(r#"{"orderId":"o","amount":1}"#);
    let resp = broker.handle_sqs_event(&event).await;
    assert_eq!(
        resp.batch_item_failures.len(),
        1,
        "State ausente deve falhar a mensagem"
    );
}

// ---------------------------------------------------------------------------
// AC: todos os extractors combinados (Json + SqsMetadata + State)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn todos_os_extractors_combinados_funcionam_juntos() {
    #[derive(Clone)]
    struct AppState {
        env: String,
    }

    let broker = Arc::new(SqsBroker::new());
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let captured = captured.clone();
        EventRouter::new()
            .with_state(AppState {
                env: "test".to_string(),
            })
            .subscribe_with(
                "orders",
                move |Json(order): Json<Order>, meta: SqsMetadata, state: State<AppState>| {
                    let captured = captured.clone();
                    async move {
                        captured.lock().unwrap().push(format!(
                            "[{}] {} mid={}",
                            state.env, order.order_id, meta.message_id
                        ));
                        Ok(())
                    }
                },
            )
    };

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert!(resp.batch_item_failures.is_empty());

    let got = captured.lock().unwrap().clone();
    assert_eq!(got.len(), 3);
    assert!(got[0].starts_with("[test] order-1 mid=11d6ee51"));
}

// ---------------------------------------------------------------------------
// AC: Json como extractor E1 (combinado com outro T explícito)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn json_como_extractor_e1_retorna_body_parsed() {
    let broker = Arc::new(SqsBroker::new());
    let captured: Arc<Mutex<Vec<Order>>> = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let captured = captured.clone();
        // Aqui Order é T (desserializado do payload), e Json<Order> é E1 (extractor)
        // — demonstra que Json<T>: FromExtractor funciona mesmo como E1
        EventRouter::new().subscribe_with("orders", move |_order: Order, body: Json<Order>| {
            let captured = captured.clone();
            async move {
                // Json<T> extrai o mesmo payload que T na posição T
                captured.lock().unwrap().push(body.0);
                Ok(())
            }
        })
    };

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert!(resp.batch_item_failures.is_empty());

    let got = captured.lock().unwrap().clone();
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].order_id, "order-1");
}

// ---------------------------------------------------------------------------
// AC: Json<T> com tipo incorreto (body válido mas tipo incompatível) → Err
// ---------------------------------------------------------------------------

#[tokio::test]
async fn json_extractor_tipo_incompativel_retorna_err() {
    #[derive(Deserialize)]
    struct StrictEvent {
        #[allow(dead_code)]
        must_exist: String,
    }

    let broker = Arc::new(SqsBroker::new());

    // fixture standard.json tem Orders (sem campo "must_exist") → falha de desserialização
    EventRouter::new()
        .subscribe_with("orders", |Json(_): Json<StrictEvent>| async { Ok(()) })
        .attach(broker.clone())
        .await
        .unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert_eq!(
        resp.batch_item_failures.len(),
        3,
        "todas as mensagens devem falhar por tipo incompatível"
    );
}

// ---------------------------------------------------------------------------
// AC: SqsMetadata retorna Err quando header SQS ausente (cobertura path de erro)
// ---------------------------------------------------------------------------

#[test]
fn sqs_metadata_err_quando_header_ausente() {
    let msg = BrokerMessage {
        topic: "orders".into(),
        partition: None,
        offset: None,
        key: None,
        payload: b"{}".to_vec(),
        headers: HashMap::new(), // sem SQS_METADATA_HEADER
        timestamp: None,
    };
    let result = SqsMetadata::from_message(&msg, None);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg_str = format!("{err}");
    assert!(
        msg_str.contains("header ausente") || msg_str.contains("SqsMetadata"),
        "mensagem de erro deve indicar header ausente, got: {msg_str}"
    );
}

#[test]
fn sqs_metadata_err_quando_header_contem_json_invalido() {
    let mut headers = HashMap::new();
    // Header presente mas com bytes inválidos para SqsMessage
    headers.insert(
        "__serverust_sqs_message".to_string(),
        b"not-valid-sqs-json".to_vec(),
    );
    let msg = BrokerMessage {
        topic: "orders".into(),
        partition: None,
        offset: None,
        key: None,
        payload: b"{}".to_vec(),
        headers,
        timestamp: None,
    };
    let result = SqsMetadata::from_message(&msg, None);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg_str = format!("{err}");
    assert!(
        msg_str.contains("failed to decode") || msg_str.contains("SqsMetadata"),
        "mensagem de erro deve indicar falha de decode, got: {msg_str}"
    );
}

// ---------------------------------------------------------------------------
// AC: BrokerError retornado pelo handler (não extractor) ainda vai para batchItemFailures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handler_err_vai_para_batch_item_failures() {
    let broker = Arc::new(SqsBroker::new());

    EventRouter::new()
        .subscribe_with("orders", |Json(order): Json<Order>| async move {
            if order.order_id == "order-2" {
                Err(BrokerError::Subscribe("handler boom".into()))
            } else {
                Ok(())
            }
        })
        .attach(broker.clone())
        .await
        .unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert_eq!(resp.batch_item_failures.len(), 1);
    assert_eq!(
        resp.batch_item_failures[0].item_identifier,
        "22d6ee51-4cc7-4302-9e22-7cd8afdaadf5"
    );
}
