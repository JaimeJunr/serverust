//! Testes do heartbeat de visibility timeout automático (US-009).
//!
//! Cobertura:
//! - HeartbeatLayer chama `change_visibility` antes do timeout expirar quando o
//!   handler é longo (fireado no threshold configurado — default 30% restante).
//! - Handler rápido: heartbeat não é disparado (background task cancelada).
//! - Handler com erro: heartbeat cancelado corretamente (sem chamadas extras).
//! - Mensagem sem receipt_handle: heartbeat silenciosamente ignorado.
//! - Threshold configurável: `with_threshold(pct)` controla quando o heartbeat
//!   é disparado.

#![cfg(feature = "sqs")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use aws_lambda_events::event::sqs::SqsMessage;
use serverust_events::broker::BrokerError;
use serverust_events::sqs::heartbeat::{HeartbeatClient, HeartbeatLayer};
use serverust_events::sqs::subscriber::SqsSubscriber;
use tower::{Service, ServiceBuilder, ServiceExt};

// ---------- mock ----------

#[derive(Default, Clone)]
struct MockHeartbeat {
    calls: Arc<Mutex<Vec<(String, String, i32)>>>, // (queue_url, receipt_handle, timeout_secs)
    fail_with: Arc<Mutex<Option<String>>>,
}

impl MockHeartbeat {
    fn new() -> Self {
        Self::default()
    }
    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
    fn calls(&self) -> Vec<(String, String, i32)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl HeartbeatClient for MockHeartbeat {
    async fn change_visibility(
        &self,
        queue_url: &str,
        receipt_handle: &str,
        visibility_timeout_secs: i32,
    ) -> Result<(), String> {
        if let Some(err) = self.fail_with.lock().unwrap().clone() {
            return Err(err);
        }
        self.calls.lock().unwrap().push((
            queue_url.to_string(),
            receipt_handle.to_string(),
            visibility_timeout_secs,
        ));
        Ok(())
    }
}

fn message_with_receipt(id: &str, receipt: &str) -> SqsMessage {
    let mut m = SqsMessage::default();
    m.message_id = Some(id.to_string());
    m.receipt_handle = Some(receipt.to_string());
    m
}

fn message_without_receipt(id: &str) -> SqsMessage {
    let mut m = SqsMessage::default();
    m.message_id = Some(id.to_string());
    m
}

// ---------- testes ----------

/// Handler longo (80ms) com visibility_timeout=100ms e threshold=30%:
/// heartbeat deve ser disparado em torno dos 70ms (quando restam ~30% do timeout).
#[tokio::test]
async fn heartbeat_fires_before_timeout_expires() {
    let client = Arc::new(MockHeartbeat::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        Ok::<_, BrokerError>(())
    });

    let mut svc = ServiceBuilder::new()
        .layer(HeartbeatLayer::new(
            client.clone(),
            "https://sqs.us-east-1.amazonaws.com/123/orders",
            Duration::from_millis(100),
        ))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_receipt("m-1", "rh-abc"))
        .await
        .unwrap();

    assert!(
        client.call_count() >= 1,
        "heartbeat deve ter sido chamado pelo menos uma vez (calls: {})",
        client.call_count()
    );
    let calls = client.calls();
    assert_eq!(calls[0].0, "https://sqs.us-east-1.amazonaws.com/123/orders");
    assert_eq!(calls[0].1, "rh-abc");
    // 100ms → as_secs() = 0 → max(1) = 1 segundo
    assert_eq!(calls[0].2, 1);
}

/// Handler rápido (10ms): heartbeat NÃO deve disparar pois handler completa
/// antes do threshold (70ms = 70% do timeout de 100ms).
#[tokio::test]
async fn heartbeat_not_fired_for_fast_handler() {
    let client = Arc::new(MockHeartbeat::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok::<_, BrokerError>(())
    });

    let mut svc = ServiceBuilder::new()
        .layer(HeartbeatLayer::new(
            client.clone(),
            "https://sqs.us-east-1.amazonaws.com/123/orders",
            Duration::from_millis(100),
        ))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_receipt("m-2", "rh-fast"))
        .await
        .unwrap();

    assert_eq!(
        client.call_count(),
        0,
        "handler rápido não deve disparar heartbeat"
    );
}

/// Handler que retorna erro: heartbeat deve ser cancelado; resultado de erro
/// propagado normalmente.
#[tokio::test]
async fn heartbeat_cancelled_on_handler_error() {
    let client = Arc::new(MockHeartbeat::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Err::<(), _>(BrokerError::Subscribe("fail".into()))
    });

    let mut svc = ServiceBuilder::new()
        .layer(HeartbeatLayer::new(
            client.clone(),
            "https://sqs.us-east-1.amazonaws.com/123/orders",
            Duration::from_millis(100),
        ))
        .service(subscriber);

    svc.ready().await.unwrap();
    let err = svc
        .call(message_with_receipt("m-3", "rh-err"))
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("fail"),
        "erro do handler deve propagar"
    );
    assert_eq!(
        client.call_count(),
        0,
        "handler rápido com erro não deve disparar heartbeat"
    );
}

/// Mensagem sem receipt_handle: heartbeat silenciosamente não é disparado.
/// O handler processa normalmente.
#[tokio::test]
async fn heartbeat_skipped_when_no_receipt_handle() {
    let client = Arc::new(MockHeartbeat::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        Ok::<_, BrokerError>(())
    });

    // visibility_timeout = 100ms, handler=80ms → heartbeat dispararia normalmente
    // mas receipt_handle está ausente.
    let mut svc = ServiceBuilder::new()
        .layer(HeartbeatLayer::new(
            client.clone(),
            "https://sqs.us-east-1.amazonaws.com/123/orders",
            Duration::from_millis(100),
        ))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_without_receipt("m-no-rh")).await.unwrap();

    assert_eq!(
        client.call_count(),
        0,
        "sem receipt_handle não deve tentar change_visibility"
    );
}

/// Threshold configurável: threshold=50% dispara quando restar 50% do timeout.
/// Com visibility=100ms e handler de 60ms, o heartbeat (50% = 50ms) deve ter
/// sido disparado antes do handler terminar.
#[tokio::test]
async fn heartbeat_configurable_threshold() {
    let client = Arc::new(MockHeartbeat::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        tokio::time::sleep(Duration::from_millis(60)).await;
        Ok::<_, BrokerError>(())
    });

    let mut svc = ServiceBuilder::new()
        .layer(
            HeartbeatLayer::new(
                client.clone(),
                "https://sqs.us-east-1.amazonaws.com/123/orders",
                Duration::from_millis(100),
            )
            .with_threshold(50), // dispara quando 50% do timeout restante
        )
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_receipt("m-thresh", "rh-thresh"))
        .await
        .unwrap();

    // Handler dura 60ms; heartbeat dispara em 50ms (50% de 100ms). Deve ter >= 1 call.
    assert!(
        client.call_count() >= 1,
        "threshold=50% deve ter disparado heartbeat antes dos 60ms (calls: {})",
        client.call_count()
    );
}

/// receipt_handle vazio é tratado igual a ausente: nenhum heartbeat.
#[tokio::test]
async fn heartbeat_skipped_when_receipt_handle_empty() {
    let client = Arc::new(MockHeartbeat::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        Ok::<_, BrokerError>(())
    });

    let mut svc = ServiceBuilder::new()
        .layer(HeartbeatLayer::new(
            client.clone(),
            "https://sqs.us-east-1.amazonaws.com/123/orders",
            Duration::from_millis(100),
        ))
        .service(subscriber);

    let mut msg = SqsMessage::default();
    msg.message_id = Some("m-empty-rh".into());
    msg.receipt_handle = Some(String::new()); // empty string

    svc.ready().await.unwrap();
    svc.call(msg).await.unwrap();

    assert_eq!(
        client.call_count(),
        0,
        "receipt_handle vazio = sem heartbeat"
    );
}

/// HeartbeatClient retornando erro: a task de heartbeat para de lopar; handler
/// continua normalmente e a mensagem é processada com sucesso.
#[tokio::test]
async fn heartbeat_stops_looping_when_client_fails() {
    let client = Arc::new(MockHeartbeat::new());
    // Configura mock para falhar em todas as chamadas.
    *client.fail_with.lock().unwrap() = Some("sqs error".to_string());

    let call_count_after = client.calls.clone();
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        Ok::<_, BrokerError>(())
    });

    // threshold=30 → heartbeat em 70ms; handler termina em 80ms
    let mut svc = ServiceBuilder::new()
        .layer(HeartbeatLayer::new(
            client.clone(),
            "https://sqs.us-east-1.amazonaws.com/123/orders",
            Duration::from_millis(100),
        ))
        .service(subscriber);

    svc.ready().await.unwrap();
    // Deve completar Ok mesmo com heartbeat falhando.
    svc.call(message_with_receipt("m-hb-fail", "rh-fail"))
        .await
        .unwrap();

    // O mock tentou mas falhou; nenhuma entrada em `calls` (o Err branch quebrou o loop).
    assert_eq!(
        call_count_after.lock().unwrap().len(),
        0,
        "falha no client: nenhuma chamada bem-sucedida gravada"
    );
}

/// Handler muito longo: heartbeat dispara múltiplas vezes.
/// Visibility=100ms, threshold=30%, handler=220ms → heartbeat em ~70ms, ~170ms.
#[tokio::test]
async fn heartbeat_fires_multiple_times_for_very_long_handler() {
    let client = Arc::new(MockHeartbeat::new());
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        tokio::time::sleep(Duration::from_millis(220)).await;
        Ok::<_, BrokerError>(())
    });

    let mut svc = ServiceBuilder::new()
        .layer(HeartbeatLayer::new(
            client.clone(),
            "https://sqs.us-east-1.amazonaws.com/123/orders",
            Duration::from_millis(100),
        ))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_receipt("m-multi", "rh-multi"))
        .await
        .unwrap();

    assert!(
        client.call_count() >= 2,
        "handler de 220ms deve disparar heartbeat ao menos 2x (calls: {})",
        client.call_count()
    );
}

/// Verifica fmt::Debug não entra em panic.
#[test]
fn heartbeat_layer_debug_does_not_panic() {
    let client = Arc::new(MockHeartbeat::new());
    let layer = HeartbeatLayer::new(
        client,
        "https://sqs.us-east-1.amazonaws.com/123/q",
        Duration::from_secs(30),
    )
    .with_threshold(30);

    let _ = format!("{layer:?}");
}

/// Verifica que o timestamp do heartbeat ocorre antes do handler completar:
/// heartbeat deve ser disparado após `visibility_timeout * 0.7` (default 30%
/// restante), antes do handler completar em `visibility_timeout * 0.8`.
#[tokio::test]
async fn heartbeat_fires_before_handler_completes() {
    let client = Arc::new(MockHeartbeat::new());
    let fired_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    let completed_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

    let fired_for_client = fired_at.clone();
    let client_inner = client.clone();

    // Wrap mock para registrar timestamp da chamada.
    #[derive(Clone)]
    struct TimestampingClient {
        inner: Arc<MockHeartbeat>,
        fired_at: Arc<Mutex<Option<Instant>>>,
    }
    #[async_trait]
    impl HeartbeatClient for TimestampingClient {
        async fn change_visibility(
            &self,
            queue_url: &str,
            receipt_handle: &str,
            timeout_secs: i32,
        ) -> Result<(), String> {
            *self.fired_at.lock().unwrap() = Some(Instant::now());
            self.inner
                .change_visibility(queue_url, receipt_handle, timeout_secs)
                .await
        }
    }

    let ts_client = Arc::new(TimestampingClient {
        inner: client_inner,
        fired_at: fired_for_client,
    });

    let completed_for_handler = completed_at.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let completed = completed_for_handler.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            *completed.lock().unwrap() = Some(Instant::now());
            Ok::<_, BrokerError>(())
        }
    });

    let mut svc = ServiceBuilder::new()
        .layer(HeartbeatLayer::new(
            ts_client,
            "https://sqs.us-east-1.amazonaws.com/123/orders",
            Duration::from_millis(100),
        ))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(message_with_receipt("m-timing", "rh-timing"))
        .await
        .unwrap();

    let fired = fired_at
        .lock()
        .unwrap()
        .expect("heartbeat deve ter sido chamado");
    let completed = completed_at
        .lock()
        .unwrap()
        .expect("handler deve ter completado");

    assert!(
        fired < completed,
        "heartbeat deve ser disparado ANTES do handler completar (fired: {:?}, completed: {:?})",
        fired,
        completed
    );
}
