//! Testes da observabilidade automática EMF + X-Ray (US-012).
//!
//! Cobre:
//! - `ObservabilityLayer` emite `serverust.sqs.messages` em todo dispatch.
//! - Emite `serverust.sqs.errors` quando o inner service retorna `Err`.
//! - Emite `serverust.sqs.latency` (Milliseconds) com `value >= 0`.
//! - Propaga X-Ray trace id a partir do atributo `AWSTraceHeader` da
//!   `SqsMessage` ou gera um novo id no formato X-Ray quando ausente.
//! - Compõe com `serverust_telemetry::tower::TracingLayer` sem regressão.
//! - Métrica `serverust.sqs.dlq_routed` (de `DlqLayer`, US-008) integra-se à
//!   mesma família EMF (mesmo namespace).

#![cfg(feature = "sqs")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_lambda_events::event::sqs::SqsMessage;
use serverust_events::broker::BrokerError;
use serverust_events::sqs::observability::{
    EMF_NAMESPACE, METRIC_ERRORS, METRIC_LATENCY, METRIC_MESSAGES, ObservabilityLayer,
    ObservabilityMetric, extract_or_generate_xray_trace_id,
};
use serverust_events::sqs::subscriber::SqsSubscriber;
use serverust_telemetry::tower::TracingLayer;
use tower::{Service, ServiceBuilder, ServiceExt};

fn message_with_id(id: &str) -> SqsMessage {
    let mut m = SqsMessage::default();
    m.message_id = Some(id.to_string());
    m.body = Some("payload".to_string());
    m.event_source_arn = Some("arn:aws:sqs:us-east-1:123456789012:orders".to_string());
    m
}

#[tokio::test]
async fn observability_layer_emits_messages_metric_on_success() {
    let recorded = Arc::new(Mutex::new(Vec::<ObservabilityMetric>::new()));
    let sink = recorded.clone();
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async { Ok::<_, BrokerError>(()) });

    let mut svc = ServiceBuilder::new()
        .layer(
            ObservabilityLayer::new("process_order").with_metric_recorder(move |m| {
                sink.lock().unwrap().push(m);
            }),
        )
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_id("m-1")).await.unwrap();

    let recs = recorded.lock().unwrap().clone();
    let messages: Vec<_> = recs.iter().filter(|m| m.name == METRIC_MESSAGES).collect();
    assert_eq!(
        messages.len(),
        1,
        "messages metric deve ser emitida uma vez"
    );
    assert_eq!(messages[0].queue, "orders");
    assert_eq!(messages[0].handler, "process_order");
    assert_eq!(messages[0].value, 1.0);
    assert_eq!(messages[0].unit, "Count");

    // Em sucesso não deve haver erro
    assert!(
        recs.iter().all(|m| m.name != METRIC_ERRORS),
        "errors metric NÃO deve ser emitida em sucesso"
    );
}

#[tokio::test]
async fn observability_layer_emits_errors_metric_on_failure() {
    let recorded = Arc::new(Mutex::new(Vec::<ObservabilityMetric>::new()));
    let sink = recorded.clone();
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async {
        Err::<(), _>(BrokerError::Subscribe("boom".into()))
    });

    let mut svc = ServiceBuilder::new()
        .layer(
            ObservabilityLayer::new("process_order").with_metric_recorder(move |m| {
                sink.lock().unwrap().push(m);
            }),
        )
        .service(subscriber);

    svc.ready().await.unwrap();
    let err = svc.call(message_with_id("m-err")).await.unwrap_err();
    assert!(err.to_string().contains("boom"), "erro propaga upstream");

    let recs = recorded.lock().unwrap().clone();
    let errors: Vec<_> = recs.iter().filter(|m| m.name == METRIC_ERRORS).collect();
    assert_eq!(errors.len(), 1, "errors metric deve ser emitida em erro");
    assert_eq!(errors[0].queue, "orders");
    assert_eq!(errors[0].handler, "process_order");
    assert_eq!(errors[0].value, 1.0);
    assert_eq!(errors[0].unit, "Count");
}

#[tokio::test]
async fn observability_layer_emits_latency_metric() {
    let recorded = Arc::new(Mutex::new(Vec::<ObservabilityMetric>::new()));
    let sink = recorded.clone();
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok::<_, BrokerError>(())
    });

    let mut svc = ServiceBuilder::new()
        .layer(
            ObservabilityLayer::new("slow_handler").with_metric_recorder(move |m| {
                sink.lock().unwrap().push(m);
            }),
        )
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_id("m-latency")).await.unwrap();

    let recs = recorded.lock().unwrap().clone();
    let latency: Vec<_> = recs.iter().filter(|m| m.name == METRIC_LATENCY).collect();
    assert_eq!(latency.len(), 1, "latency metric deve ser emitida");
    assert_eq!(latency[0].unit, "Milliseconds");
    assert!(
        latency[0].value >= 5.0,
        "latência deve refletir o sleep do handler (>=5ms, got {})",
        latency[0].value
    );
}

#[tokio::test]
async fn observability_layer_emits_latency_even_on_error() {
    let recorded = Arc::new(Mutex::new(Vec::<ObservabilityMetric>::new()));
    let sink = recorded.clone();
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async {
        Err::<(), _>(BrokerError::Subscribe("nope".into()))
    });

    let mut svc = ServiceBuilder::new()
        .layer(ObservabilityLayer::new("h").with_metric_recorder(move |m| {
            sink.lock().unwrap().push(m);
        }))
        .service(subscriber);

    svc.ready().await.unwrap();
    let _ = svc.call(message_with_id("m-1")).await;

    let recs = recorded.lock().unwrap().clone();
    assert!(
        recs.iter().any(|m| m.name == METRIC_LATENCY),
        "latency deve ser emitida mesmo quando o handler falha"
    );
}

#[test]
fn extract_xray_trace_id_parses_aws_trace_header_root() {
    let mut m = SqsMessage::default();
    let mut attrs = HashMap::new();
    attrs.insert(
        "AWSTraceHeader".to_string(),
        "Root=1-65aab02e-1c0d2c0c1c0d2c0c1c0d2c0c;Parent=53995c3f42cd8ad8;Sampled=1".to_string(),
    );
    m.attributes = attrs;

    let id = extract_or_generate_xray_trace_id(&m);
    assert_eq!(id, "1-65aab02e-1c0d2c0c1c0d2c0c1c0d2c0c");
}

#[test]
fn extract_xray_trace_id_generates_when_attribute_absent() {
    let m = SqsMessage::default();
    let id = extract_or_generate_xray_trace_id(&m);
    // Formato X-Ray: 1-<8 hex>-<24 hex>
    assert!(
        id.starts_with("1-"),
        "trace id deve seguir formato X-Ray: {id}"
    );
    let parts: Vec<&str> = id.split('-').collect();
    assert_eq!(parts.len(), 3, "trace id mal-formado: {id}");
    assert_eq!(parts[0], "1");
    assert_eq!(parts[1].len(), 8, "epoch deve ter 8 hex chars: {id}");
    assert_eq!(parts[2].len(), 24, "random deve ter 24 hex chars: {id}");
}

#[test]
fn extract_xray_trace_id_returns_full_header_if_root_absent() {
    let mut m = SqsMessage::default();
    let mut attrs = HashMap::new();
    attrs.insert(
        "AWSTraceHeader".to_string(),
        "Sampled=0".to_string(), // sem Root
    );
    m.attributes = attrs;

    let id = extract_or_generate_xray_trace_id(&m);
    assert_eq!(
        id, "Sampled=0",
        "se Root= não está presente, retorna header inteiro"
    );
}

#[tokio::test]
async fn observability_composes_with_telemetry_tracing_layer() {
    // Garantia: a integração com TracingLayer (serverust-telemetry) compõe
    // sem alterações — span tracing fica como camada externa, observability
    // emite métricas dentro do span.
    let recorded = Arc::new(Mutex::new(Vec::<ObservabilityMetric>::new()));
    let sink = recorded.clone();
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async { Ok::<_, BrokerError>(()) });

    let mut svc = ServiceBuilder::new()
        .layer(TracingLayer::new("sqs.orders"))
        .layer(
            ObservabilityLayer::new("process_order").with_metric_recorder(move |m| {
                sink.lock().unwrap().push(m);
            }),
        )
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_id("m-c")).await.unwrap();

    let recs = recorded.lock().unwrap().clone();
    assert!(recs.iter().any(|m| m.name == METRIC_MESSAGES));
    assert!(recs.iter().any(|m| m.name == METRIC_LATENCY));
}

#[tokio::test]
async fn observability_extracts_queue_from_event_source_arn() {
    let recorded = Arc::new(Mutex::new(Vec::<ObservabilityMetric>::new()));
    let sink = recorded.clone();
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async { Ok::<_, BrokerError>(()) });

    let mut svc = ServiceBuilder::new()
        .layer(ObservabilityLayer::new("h").with_metric_recorder(move |m| {
            sink.lock().unwrap().push(m);
        }))
        .service(subscriber);

    let mut m = SqsMessage::default();
    m.message_id = Some("m-1".into());
    m.event_source_arn = Some("arn:aws:sqs:us-east-1:000000000000:wallet-events".into());

    svc.ready().await.unwrap();
    svc.call(m).await.unwrap();

    let recs = recorded.lock().unwrap().clone();
    assert!(
        recs.iter().all(|m| m.queue == "wallet-events"),
        "queue extraída do segmento final do ARN: {recs:?}"
    );
}

#[tokio::test]
async fn observability_uses_unknown_queue_when_arn_absent() {
    let recorded = Arc::new(Mutex::new(Vec::<ObservabilityMetric>::new()));
    let sink = recorded.clone();
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async { Ok::<_, BrokerError>(()) });

    let mut svc = ServiceBuilder::new()
        .layer(ObservabilityLayer::new("h").with_metric_recorder(move |m| {
            sink.lock().unwrap().push(m);
        }))
        .service(subscriber);

    let mut m = SqsMessage::default();
    m.message_id = Some("m-1".into());
    m.event_source_arn = None;

    svc.ready().await.unwrap();
    svc.call(m).await.unwrap();

    let recs = recorded.lock().unwrap().clone();
    assert!(
        recs.iter().all(|m| m.queue == "unknown"),
        "queue sem ARN é rotulada como 'unknown': {recs:?}"
    );
}

#[test]
fn emf_namespace_and_metric_names_are_canonical() {
    // Critério de aceite: nomes exatos das métricas e namespace estável.
    assert_eq!(EMF_NAMESPACE, "serverust.sqs");
    assert_eq!(METRIC_MESSAGES, "serverust.sqs.messages");
    assert_eq!(METRIC_ERRORS, "serverust.sqs.errors");
    assert_eq!(METRIC_LATENCY, "serverust.sqs.latency");
    // dlq_routed permanece em DlqLayer (US-008) — referenciamos seu nome
    // canônico para garantir que a família EMF está completa.
    assert_eq!(
        serverust_events::sqs::layers::DLQ_ROUTED_METRIC,
        "serverust.sqs.dlq_routed"
    );
}
