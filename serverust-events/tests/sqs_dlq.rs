//! Testes da política DLQ + retry declarativos (US-008).
//!
//! Cobertura:
//! - `DlqLayer` envelopa um `Service<SqsMessage>`: em sucesso passa direto;
//!   em erro roteia para a DLQ com `_serverust_failure_reason`.
//! - `RetryLayer::with_backoff(base, max_total)` aplica backoff exponencial
//!   entre tentativas, respeitando o teto `max_total` (= visibility timeout).
//! - Composição: `RetryLayer(max=6) → handler` envolvido por `DlqLayer` produz
//!   o caminho "mensagem falhando 6x terminando em DLQ".
//! - Métrica `serverust.sqs.dlq_routed` é emitida com dimensões `queue` e
//!   `handler` quando a mensagem é roteada.

#![cfg(feature = "sqs")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use aws_lambda_events::event::sqs::SqsMessage;
use serverust_events::broker::BrokerError;
use serverust_events::sqs::layers::{DlqClient, DlqLayer, FAILURE_REASON_ATTR, RetryLayer};
use serverust_events::sqs::subscriber::SqsSubscriber;
use tower::{Service, ServiceBuilder, ServiceExt};

fn message_with_body(id: &str, body: &str) -> SqsMessage {
    let mut m = SqsMessage::default();
    m.message_id = Some(id.to_string());
    m.body = Some(body.to_string());
    m
}

#[derive(Default)]
struct CapturedSend {
    queue: String,
    body: String,
    attributes: HashMap<String, String>,
}

#[derive(Default, Clone)]
struct MockDlqClient {
    sent: Arc<Mutex<Vec<CapturedSend>>>,
    fail_with: Arc<Mutex<Option<String>>>,
}

impl MockDlqClient {
    fn new() -> Self {
        Self::default()
    }
    fn sent(&self) -> Vec<CapturedSend> {
        std::mem::take(&mut *self.sent.lock().unwrap())
    }
    fn calls(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

#[async_trait]
impl DlqClient for MockDlqClient {
    async fn send_to_dlq(
        &self,
        queue: &str,
        body: &str,
        attributes: HashMap<String, String>,
    ) -> Result<(), String> {
        if let Some(err) = self.fail_with.lock().unwrap().clone() {
            return Err(err);
        }
        self.sent.lock().unwrap().push(CapturedSend {
            queue: queue.to_string(),
            body: body.to_string(),
            attributes,
        });
        Ok(())
    }
}

#[tokio::test]
async fn dlq_layer_passes_through_on_success() {
    let dlq = Arc::new(MockDlqClient::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move { Ok::<_, BrokerError>(()) });

    let mut svc = ServiceBuilder::new()
        .layer(DlqLayer::new(dlq.clone(), "orders-dlq", "process_order"))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_body("m-1", "x")).await.unwrap();
    assert_eq!(dlq.calls(), 0, "sucesso não deve rotear para DLQ");
}

#[tokio::test]
async fn dlq_layer_routes_to_dlq_on_handler_error() {
    let dlq = Arc::new(MockDlqClient::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        Err::<(), _>(BrokerError::Subscribe("boom".into()))
    });

    let mut svc = ServiceBuilder::new()
        .layer(DlqLayer::new(dlq.clone(), "orders-dlq", "process_order"))
        .service(subscriber);

    svc.ready().await.unwrap();
    // DlqLayer convertido o Err -> Ok após rotear para DLQ.
    svc.call(message_with_body("m-1", "payload"))
        .await
        .expect("DlqLayer deve converter erro em Ok após rotear");

    let sent = dlq.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].queue, "orders-dlq");
    assert_eq!(sent[0].body, "payload");
    assert!(
        sent[0]
            .attributes
            .get(FAILURE_REASON_ATTR)
            .map(|v| v.contains("boom"))
            .unwrap_or(false),
        "atributo _serverust_failure_reason deve carregar o motivo: {:?}",
        sent[0].attributes
    );
}

#[tokio::test]
async fn dlq_send_failure_propagates_error_upstream() {
    let dlq = Arc::new(MockDlqClient::new());
    *dlq.fail_with.lock().unwrap() = Some("dlq down".to_string());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        Err::<(), _>(BrokerError::Subscribe("boom".into()))
    });

    let mut svc = ServiceBuilder::new()
        .layer(DlqLayer::new(dlq.clone(), "orders-dlq", "process_order"))
        .service(subscriber);

    svc.ready().await.unwrap();
    let err = svc
        .call(message_with_body("m-1", "payload"))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("dlq down"),
        "falha do DlqClient deve propagar como erro para SQS retry: {err}"
    );
}

#[tokio::test]
async fn dlq_layer_skips_when_message_body_absent() {
    let dlq = Arc::new(MockDlqClient::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        Err::<(), _>(BrokerError::Subscribe("boom".into()))
    });

    let mut svc = ServiceBuilder::new()
        .layer(DlqLayer::new(dlq.clone(), "orders-dlq", "process_order"))
        .service(subscriber);

    let mut msg = SqsMessage::default();
    msg.message_id = Some("m-no-body".into());
    msg.body = None;

    svc.ready().await.unwrap();
    svc.call(msg).await.unwrap();

    let sent = dlq.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].body, "", "mensagem sem body envia corpo vazio");
}

#[tokio::test]
async fn retry_layer_exponential_backoff_accumulates_delay() {
    // Com base=10ms zero-cost na suite, basta termos backoff aplicado entre
    // tentativas: 3 tentativas com base=10ms => 1ª sem espera, 2ª espera 10ms,
    // 3ª espera 20ms. Total >= 30ms. Confiamos no relógio monotônico.
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Err::<(), _>(BrokerError::Subscribe("fail".into()))
        }
    });

    let mut svc = ServiceBuilder::new()
        .layer(RetryLayer::new(3).with_backoff(Duration::from_millis(10), Duration::from_secs(30)))
        .service(subscriber);

    let started = std::time::Instant::now();
    svc.ready().await.unwrap();
    let _ = svc.call(message_with_body("m-1", "x")).await;
    let elapsed = started.elapsed();

    assert_eq!(*calls.lock().unwrap(), 3, "deve tentar 3 vezes");
    assert!(
        elapsed >= Duration::from_millis(28),
        "backoff exponencial 10ms+20ms deve adicionar >=28ms (elapsed: {elapsed:?})"
    );
}

#[tokio::test]
async fn retry_backoff_capped_by_max_total_visibility() {
    // Backoff exponencial 100ms base com max=10 atingiria 100+200+400+...+25600ms.
    // Com max_total=50ms, o segundo retry deve esperar no máximo o que sobra (<=50ms total).
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Err::<(), _>(BrokerError::Subscribe("fail".into()))
        }
    });

    // 5 attempts c/ base=100ms teoricamente => 100+200+400+800 ms ≈ 1500ms total.
    // Mas com cap em 50ms, nunca deve esperar mais que 50ms acumulado.
    let mut svc = ServiceBuilder::new()
        .layer(
            RetryLayer::new(5).with_backoff(Duration::from_millis(100), Duration::from_millis(50)),
        )
        .service(subscriber);

    let started = std::time::Instant::now();
    svc.ready().await.unwrap();
    let _ = svc.call(message_with_body("m-1", "x")).await;
    let elapsed = started.elapsed();

    assert_eq!(*calls.lock().unwrap(), 5);
    assert!(
        elapsed < Duration::from_millis(500),
        "max_total=50ms deve impedir backoff total ≥ 500ms (elapsed: {elapsed:?})"
    );
}

#[tokio::test]
async fn message_failing_six_times_ends_in_dlq() {
    // Critério canônico: "mensagem falhando 6x terminando em DLQ".
    let attempts = Arc::new(Mutex::new(0_u32));
    let attempts_for_handler = attempts.clone();
    let dlq = Arc::new(MockDlqClient::new());

    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let attempts = attempts_for_handler.clone();
        async move {
            *attempts.lock().unwrap() += 1;
            Err::<(), _>(BrokerError::Subscribe(format!(
                "attempt {} failed",
                attempts.lock().unwrap()
            )))
        }
    });

    let mut svc = ServiceBuilder::new()
        .layer(DlqLayer::new(dlq.clone(), "orders-dlq", "process_order"))
        .layer(RetryLayer::new(6).with_backoff(Duration::ZERO, Duration::from_secs(30)))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_body("m-fail", "order#1"))
        .await
        .expect("DlqLayer absorve a falha final");

    assert_eq!(*attempts.lock().unwrap(), 6, "RetryLayer deve tentar 6x");
    let sent = dlq.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].queue, "orders-dlq");
    assert_eq!(sent[0].body, "order#1");
    assert!(
        sent[0]
            .attributes
            .get(FAILURE_REASON_ATTR)
            .map(|v| v.contains("attempt"))
            .unwrap_or(false),
        "_serverust_failure_reason deve preservar o motivo"
    );
}

#[tokio::test]
async fn dlq_layer_emits_emf_metric_on_routing() {
    // Critério: "Metrica serverust.sqs.dlq_routed por queue/handler".
    let dlq = Arc::new(MockDlqClient::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        Err::<(), _>(BrokerError::Subscribe("boom".into()))
    });

    let recorded = Arc::new(Mutex::new(
        Vec::<serverust_events::sqs::layers::DlqMetric>::new(),
    ));
    let recorded_for_layer = recorded.clone();

    let mut svc = ServiceBuilder::new()
        .layer(
            DlqLayer::new(dlq.clone(), "orders-dlq", "process_order").with_metric_recorder(
                move |m: serverust_events::sqs::layers::DlqMetric| {
                    recorded_for_layer.lock().unwrap().push(m);
                },
            ),
        )
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_body("m-1", "payload")).await.unwrap();

    let recs = recorded.lock().unwrap().clone();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].metric_name, "serverust.sqs.dlq_routed");
    assert_eq!(recs[0].queue, "orders-dlq");
    assert_eq!(recs[0].handler, "process_order");
    assert_eq!(recs[0].value, 1.0);
}
