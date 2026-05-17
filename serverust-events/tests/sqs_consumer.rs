//! Testes para `SqsBroker` (US-001) — broker sink-only que despacha
//! `aws_lambda_events::event::sqs::SqsEvent` para handlers inscritos e
//! produz `SqsBatchResponse` com `batchItemFailures` para mensagens com
//! falha (modelo de partial batch failure do Lambda ESM).
//!
//! Atrás de `feature = "sqs"`; gate verificado em outros testes.

#![cfg(feature = "sqs")]

use std::sync::Arc;
use std::sync::Mutex;

use aws_lambda_events::event::sqs::SqsEvent;
use serde::Deserialize;
use serverust_events::broker::{Broker, BrokerError};
use serverust_events::router::EventRouter;
use serverust_events::sqs::consumer::SqsBroker;
use serverust_events::sqs::extract::SqsMetadata;

fn fixture() -> SqsEvent {
    let raw = include_str!("fixtures/sqs/standard.json");
    serde_json::from_str(raw).expect("fixture standard.json deve ser SqsEvent valido")
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
struct Order {
    #[serde(rename = "orderId")]
    order_id: String,
    amount: u64,
}

#[tokio::test]
async fn handle_sqs_event_despacha_corpos_para_handler_inscrito() {
    let broker = Arc::new(SqsBroker::new());
    let received: Arc<Mutex<Vec<Order>>> = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let received = received.clone();
        EventRouter::new().subscribe::<Order, _, _>("orders", move |event| {
            let received = received.clone();
            async move {
                received.lock().unwrap().push(event);
                Ok(())
            }
        })
    };

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert!(
        resp.batch_item_failures.is_empty(),
        "todas mensagens succeeded => batch_item_failures vazio, got: {:?}",
        resp.batch_item_failures
    );

    let got = received.lock().unwrap().clone();
    assert_eq!(got.len(), 3);
    assert_eq!(
        got[0],
        Order {
            order_id: "order-1".into(),
            amount: 100
        }
    );
}

#[tokio::test]
async fn handle_sqs_event_falha_individual_vira_batch_item_failure() {
    let broker = Arc::new(SqsBroker::new());

    let router = EventRouter::new().subscribe::<Order, _, _>("orders", |order: Order| async move {
        if order.order_id == "order-2" {
            Err(BrokerError::Subscribe(format!(
                "boom em {}",
                order.order_id
            )))
        } else {
            Ok(())
        }
    });

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert_eq!(
        resp.batch_item_failures.len(),
        1,
        "{:?}",
        resp.batch_item_failures
    );
    assert_eq!(
        resp.batch_item_failures[0].item_identifier,
        "22d6ee51-4cc7-4302-9e22-7cd8afdaadf5"
    );
}

#[tokio::test]
async fn handle_sqs_event_todas_falhas_aparecem_em_batch_item_failures() {
    let broker = Arc::new(SqsBroker::new());

    let router = EventRouter::new()
        .subscribe::<Order, _, _>("orders", |_order: Order| async move {
            Err(BrokerError::Subscribe("always fail".into()))
        });

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert_eq!(resp.batch_item_failures.len(), 3);
    let ids: Vec<String> = resp
        .batch_item_failures
        .iter()
        .map(|f| f.item_identifier.clone())
        .collect();
    assert!(ids.iter().any(|i| i.starts_with("11d6ee51")));
    assert!(ids.iter().any(|i| i.starts_with("22d6ee51")));
    assert!(ids.iter().any(|i| i.starts_with("33d6ee51")));
}

#[tokio::test]
async fn handle_sqs_event_ignora_fila_sem_subscriber_sem_falhar() {
    let broker = Arc::new(SqsBroker::new());
    // Nenhum subscriber registrado para "orders" — mensagens sao silenciosamente
    // ignoradas (nao geram batch_item_failures). Em producao isso seria uma
    // misconfiguracao do ESM, mas o broker nao tem como saber.
    let resp = broker.handle_sqs_event(&fixture()).await;
    assert!(resp.batch_item_failures.is_empty());
}

#[tokio::test]
async fn sqs_broker_publish_falha_indicando_sink_only() {
    let broker = SqsBroker::new();
    let err = broker.publish("any.queue", b"payload").await.unwrap_err();
    let s = format!("{err}");
    assert!(s.contains("SqsBroker"), "msg foi: {s}");
}

#[tokio::test]
async fn handle_sqs_event_ignora_mensagem_sem_event_source_arn() {
    let broker = Arc::new(SqsBroker::new());
    let called: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));

    let router = {
        let called = called.clone();
        EventRouter::new().subscribe::<Order, _, _>("orders", move |_| {
            let called = called.clone();
            async move {
                *called.lock().unwrap() += 1;
                Ok(())
            }
        })
    };
    router.attach(broker.clone()).await.unwrap();

    // SqsEvent com 1 mensagem sem event_source_arn.
    let raw = r#"{
        "Records": [{
            "messageId": "no-arn-1",
            "receiptHandle": "x",
            "body": "{\"orderId\":\"o\",\"amount\":1}",
            "attributes": {},
            "messageAttributes": {}
        }]
    }"#;
    let event: SqsEvent = serde_json::from_str(raw).unwrap();
    let resp = broker.handle_sqs_event(&event).await;
    assert!(resp.batch_item_failures.is_empty());
    assert_eq!(
        *called.lock().unwrap(),
        0,
        "handler nao deve ter sido chamado"
    );
}

#[tokio::test]
async fn handle_sqs_event_falha_sem_message_id_apenas_loga_sem_falha_no_batch() {
    let broker = Arc::new(SqsBroker::new());

    let router = EventRouter::new().subscribe::<Order, _, _>("orders", |_| async move {
        Err(BrokerError::Subscribe("boom".into()))
    });
    router.attach(broker.clone()).await.unwrap();

    let raw = r#"{
        "Records": [{
            "receiptHandle": "x",
            "body": "{\"orderId\":\"o\",\"amount\":1}",
            "attributes": {},
            "messageAttributes": {},
            "eventSourceARN": "arn:aws:sqs:us-east-1:1:orders",
            "eventSource": "aws:sqs"
        }]
    }"#;
    let event: SqsEvent = serde_json::from_str(raw).unwrap();
    let resp = broker.handle_sqs_event(&event).await;
    // Sem message_id, nao ha como reportar no batchItemFailures.
    assert!(resp.batch_item_failures.is_empty());
}

#[tokio::test]
async fn sqs_broker_subscribed_queues_lista_filas_em_ordem() {
    let broker = SqsBroker::new();
    broker
        .subscribe("orders", Arc::new(|_| Box::pin(async { Ok(()) })))
        .await
        .unwrap();
    broker
        .subscribe("billing", Arc::new(|_| Box::pin(async { Ok(()) })))
        .await
        .unwrap();
    assert_eq!(
        broker.subscribed_queues(),
        vec!["orders".to_string(), "billing".to_string()]
    );
}

fn fixture_10() -> SqsEvent {
    let raw = include_str!("fixtures/sqs/batch_10.json");
    serde_json::from_str(raw).expect("fixture batch_10.json deve ser SqsEvent valido")
}

/// US-003 — Lambda ESM: sucesso nao aparece em batchItemFailures; erro aparece.
/// Batch de 10 mensagens: IDs pares falham, impares tem sucesso.
#[tokio::test]
async fn handle_sqs_event_batch_de_10_sucesso_e_erro_misturados() {
    let broker = Arc::new(SqsBroker::new());

    let router = EventRouter::new().subscribe::<Order, _, _>("orders", |order: Order| async move {
        let num: u32 = order.order_id.trim_start_matches("order-").parse().unwrap();
        if num % 2 == 0 {
            Err(BrokerError::Subscribe(format!(
                "falha em {}",
                order.order_id
            )))
        } else {
            Ok(())
        }
    });
    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture_10()).await;

    assert_eq!(
        resp.batch_item_failures.len(),
        5,
        "{:?}",
        resp.batch_item_failures
    );
    let ids: std::collections::HashSet<String> = resp
        .batch_item_failures
        .iter()
        .map(|f| f.item_identifier.clone())
        .collect();
    for expected in ["msg-02", "msg-04", "msg-06", "msg-08", "msg-10"] {
        assert!(
            ids.contains(expected),
            "{expected} deve estar em batchItemFailures"
        );
    }
    for not_expected in ["msg-01", "msg-03", "msg-05", "msg-07", "msg-09"] {
        assert!(
            !ids.contains(not_expected),
            "{not_expected} nao deve estar em batchItemFailures"
        );
    }
}

#[test]
fn sqs_broker_default_equivale_a_new() {
    let broker = SqsBroker::default();
    assert!(broker.subscribed_queues().is_empty());
}

#[tokio::test]
async fn sqs_broker_implementa_broker_trait_object() {
    let broker: Arc<dyn Broker> = Arc::new(SqsBroker::new());
    let router = EventRouter::new().subscribe::<Order, _, _>("orders", |_| async { Ok(()) });
    router.attach(broker).await.unwrap();
}

#[tokio::test]
async fn handler_pode_extrair_sqs_metadata_via_extractor() {
    let broker = Arc::new(SqsBroker::new());
    let captured: Arc<Mutex<Vec<(String, String, String)>>> = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let captured = captured.clone();
        EventRouter::new().subscribe_with("orders", move |order: Order, meta: SqsMetadata| {
            let captured = captured.clone();
            async move {
                let trace = meta
                    .message_attributes
                    .get("trace_id")
                    .and_then(|a| a.string_value.clone())
                    .unwrap_or_default();
                captured
                    .lock()
                    .unwrap()
                    .push((order.order_id, meta.message_id, trace));
                Ok(())
            }
        })
    };

    router.attach(broker.clone()).await.unwrap();

    let resp = broker.handle_sqs_event(&fixture()).await;
    assert!(resp.batch_item_failures.is_empty());

    let got = captured.lock().unwrap().clone();
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].0, "order-1");
    assert_eq!(got[0].1, "11d6ee51-4cc7-4302-9e22-7cd8afdaadf5");
    assert_eq!(got[0].2, "abc-123");
    // mensagem 2 nao tem trace_id => empty string
    assert_eq!(got[1].2, "");
}

#[tokio::test]
async fn handler_pode_extrair_state_compartilhado() {
    #[derive(Clone)]
    struct AppState {
        prefix: String,
    }

    let broker = Arc::new(SqsBroker::new());
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let captured = captured.clone();
        EventRouter::new()
            .with_state(AppState {
                prefix: "OK::".into(),
            })
            .subscribe_with(
                "orders",
                move |order: Order, state: serverust_events::extract::State<AppState>| {
                    let captured = captured.clone();
                    async move {
                        captured
                            .lock()
                            .unwrap()
                            .push(format!("{}{}", state.prefix, order.order_id));
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
        vec![
            "OK::order-1".to_string(),
            "OK::order-2".to_string(),
            "OK::order-3".to_string()
        ]
    );
}
