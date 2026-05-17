//! Testes para `serverust_events::sqs::standalone::StandaloneSqsBroker` (US-010).
//!
//! Broker long-poll para ECS/EC2: usa `ReceiveMessage` em loop com
//! `WaitTimeSeconds=20`, despacha cada mensagem para os handlers inscritos
//! e deleta as mensagens processadas com sucesso. Compartilha o `Broker`
//! trait com `SqsBroker` (Lambda ESM), portanto a mesma macro `#[subscriber]`
//! funciona em ambos os modos sem mudar código de negócio.
//!
//! Estes testes usam mocks in-memory para `ReceiveClient` e `DeleteClient`:
//! nenhum acesso à AWS real.

#![cfg(feature = "sqs")]

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use aws_lambda_events::event::sqs::SqsMessage;
use serde::Deserialize;
use serverust_events::broker::{Broker, BrokerError};
use serverust_events::router::EventRouter;
use serverust_events::sqs::delete::{DeleteClient, DeleteEntry, DeleteResult};
use serverust_events::sqs::standalone::{
    ReceiveClient, ReceiveResult, StandaloneConfig, StandaloneSqsBroker,
};

// --------------------------------------------------------------------------
// Mocks
// --------------------------------------------------------------------------

/// Mock que entrega uma fila pré-carregada de batches. Cada chamada
/// `receive(...)` consome o próximo batch (FIFO). Quando vazio, retorna
/// `Ok(empty)` após um sleep curto (simula long-poll vazio).
struct MockReceiveClient {
    batches: Mutex<Vec<Vec<SqsMessage>>>,
    calls: AtomicU32,
    last_wait_time: Mutex<i32>,
    last_max_messages: Mutex<i32>,
    error_count: AtomicU32,
    fail_first_n: u32,
}

impl MockReceiveClient {
    fn new(batches: Vec<Vec<SqsMessage>>) -> Self {
        Self {
            batches: Mutex::new(batches),
            calls: AtomicU32::new(0),
            last_wait_time: Mutex::new(0),
            last_max_messages: Mutex::new(0),
            error_count: AtomicU32::new(0),
            fail_first_n: 0,
        }
    }

    fn with_failures(mut self, n: u32) -> Self {
        self.fail_first_n = n;
        self
    }

    fn call_count(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }

    fn last_wait_time(&self) -> i32 {
        *self.last_wait_time.lock().unwrap()
    }

    fn last_max_messages(&self) -> i32 {
        *self.last_max_messages.lock().unwrap()
    }

    fn error_count(&self) -> u32 {
        self.error_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ReceiveClient for MockReceiveClient {
    async fn receive(
        &self,
        _queue_url: &str,
        max_messages: i32,
        wait_time_seconds: i32,
    ) -> Result<ReceiveResult, String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_wait_time.lock().unwrap() = wait_time_seconds;
        *self.last_max_messages.lock().unwrap() = max_messages;

        if n < self.fail_first_n {
            self.error_count.fetch_add(1, Ordering::SeqCst);
            return Err(format!("simulated receive error #{n}"));
        }

        let next = {
            let mut b = self.batches.lock().unwrap();
            if b.is_empty() {
                None
            } else {
                Some(b.remove(0))
            }
        };
        match next {
            Some(messages) => Ok(ReceiveResult { messages }),
            None => {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(ReceiveResult { messages: vec![] })
            }
        }
    }
}

/// Mock simples de DeleteClient — registra todas as entradas deletadas.
struct MockDeleteClient {
    deleted: Mutex<Vec<DeleteEntry>>,
}

impl MockDeleteClient {
    fn new() -> Self {
        Self {
            deleted: Mutex::new(Vec::new()),
        }
    }

    fn deleted_ids(&self) -> Vec<String> {
        self.deleted
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.id.clone())
            .collect()
    }
}

#[async_trait]
impl DeleteClient for MockDeleteClient {
    async fn delete_batch(
        &self,
        _queue_url: &str,
        entries: Vec<DeleteEntry>,
    ) -> Result<DeleteResult, String> {
        self.deleted.lock().unwrap().extend(entries);
        Ok(DeleteResult { failed: vec![] })
    }
}

// --------------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------------

fn build_message(id: &str, body: &str) -> SqsMessage {
    let mut m = SqsMessage::default();
    m.message_id = Some(id.to_string());
    m.receipt_handle = Some(format!("rh-{id}"));
    m.body = Some(body.to_string());
    m.event_source_arn = Some("arn:aws:sqs:us-east-1:123456789012:orders".to_string());
    m
}

const QUEUE_URL: &str = "https://sqs.us-east-1.amazonaws.com/123456789012/orders";
const QUEUE_NAME: &str = "orders";

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
struct Order {
    #[serde(rename = "orderId")]
    order_id: String,
    amount: u64,
}

// --------------------------------------------------------------------------
// Testes
// --------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn run_long_poll_passes_default_wait_time_seconds_20() {
    let receive = Arc::new(MockReceiveClient::new(vec![]));
    let delete = Arc::new(MockDeleteClient::new());
    let broker = Arc::new(StandaloneSqsBroker::new(
        receive.clone(),
        delete.clone(),
        QUEUE_URL.to_string(),
        QUEUE_NAME.to_string(),
    ));

    let broker_run = broker.clone();
    let handle = tokio::spawn(async move { broker_run.run().await });

    // Espera ao menos uma chamada
    tokio::time::sleep(Duration::from_millis(100)).await;
    broker.signal_shutdown();
    handle.await.unwrap().unwrap();

    assert!(
        receive.call_count() >= 1,
        "esperava ao menos 1 chamada de receive"
    );
    assert_eq!(
        receive.last_wait_time(),
        20,
        "WaitTimeSeconds deve ser 20 por default (long-poll)"
    );
    assert_eq!(
        receive.last_max_messages(),
        10,
        "MaxNumberOfMessages deve ser 10 por default"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn run_respects_custom_wait_time_seconds_config() {
    let receive = Arc::new(MockReceiveClient::new(vec![]));
    let delete = Arc::new(MockDeleteClient::new());
    let cfg = StandaloneConfig {
        wait_time_seconds: 5,
        max_messages: 3,
        ..Default::default()
    };
    let broker = Arc::new(
        StandaloneSqsBroker::new(
            receive.clone(),
            delete.clone(),
            QUEUE_URL.to_string(),
            QUEUE_NAME.to_string(),
        )
        .with_config(cfg),
    );

    let broker_run = broker.clone();
    let handle = tokio::spawn(async move { broker_run.run().await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    broker.signal_shutdown();
    handle.await.unwrap().unwrap();

    assert_eq!(receive.last_wait_time(), 5);
    assert_eq!(receive.last_max_messages(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn run_dispatches_messages_to_subscribed_handler_via_router() {
    let batch = vec![
        build_message("m1", r#"{"orderId":"order-1","amount":10}"#),
        build_message("m2", r#"{"orderId":"order-2","amount":20}"#),
    ];

    let receive = Arc::new(MockReceiveClient::new(vec![batch]));
    let delete = Arc::new(MockDeleteClient::new());
    let broker = Arc::new(StandaloneSqsBroker::new(
        receive.clone(),
        delete.clone(),
        QUEUE_URL.to_string(),
        QUEUE_NAME.to_string(),
    ));

    let received: Arc<Mutex<Vec<Order>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    EventRouter::new()
        .subscribe::<Order, _, _>(QUEUE_NAME, move |event: Order| {
            let received = received_clone.clone();
            async move {
                received.lock().unwrap().push(event);
                Ok(())
            }
        })
        .attach(broker.clone())
        .await
        .unwrap();

    let broker_run = broker.clone();
    let handle = tokio::spawn(async move { broker_run.run().await });

    // Aguarda processamento das mensagens
    for _ in 0..50 {
        if received.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    broker.signal_shutdown();
    handle.await.unwrap().unwrap();

    let got = received.lock().unwrap().clone();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].order_id, "order-1");
    assert_eq!(got[1].order_id, "order-2");
}

#[tokio::test(flavor = "current_thread")]
async fn run_deletes_messages_after_successful_handling() {
    let batch = vec![
        build_message("m1", r#"{"orderId":"o-1","amount":10}"#),
        build_message("m2", r#"{"orderId":"o-2","amount":20}"#),
    ];

    let receive = Arc::new(MockReceiveClient::new(vec![batch]));
    let delete = Arc::new(MockDeleteClient::new());
    let broker = Arc::new(StandaloneSqsBroker::new(
        receive.clone(),
        delete.clone(),
        QUEUE_URL.to_string(),
        QUEUE_NAME.to_string(),
    ));

    EventRouter::new()
        .subscribe::<Order, _, _>(QUEUE_NAME, |_o: Order| async move { Ok(()) })
        .attach(broker.clone())
        .await
        .unwrap();

    let broker_run = broker.clone();
    let handle = tokio::spawn(async move { broker_run.run().await });

    for _ in 0..50 {
        if delete.deleted_ids().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    broker.signal_shutdown();
    handle.await.unwrap().unwrap();

    let mut ids = delete.deleted_ids();
    ids.sort();
    assert_eq!(ids, vec!["m1".to_string(), "m2".to_string()]);
}

#[tokio::test(flavor = "current_thread")]
async fn run_does_not_delete_when_handler_fails() {
    let batch = vec![build_message("bad", r#"{"orderId":"x","amount":99}"#)];

    let receive = Arc::new(MockReceiveClient::new(vec![batch]));
    let delete = Arc::new(MockDeleteClient::new());
    let broker = Arc::new(StandaloneSqsBroker::new(
        receive.clone(),
        delete.clone(),
        QUEUE_URL.to_string(),
        QUEUE_NAME.to_string(),
    ));

    EventRouter::new()
        .subscribe::<Order, _, _>(QUEUE_NAME, |_o: Order| async move {
            Err(BrokerError::Subscribe("boom".into()))
        })
        .attach(broker.clone())
        .await
        .unwrap();

    let broker_run = broker.clone();
    let handle = tokio::spawn(async move { broker_run.run().await });

    tokio::time::sleep(Duration::from_millis(200)).await;
    broker.signal_shutdown();
    handle.await.unwrap().unwrap();

    assert!(
        delete.deleted_ids().is_empty(),
        "handler que falha NÃO deve disparar DeleteMessageBatch — visibility timeout devolve a msg"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn run_graceful_shutdown_drains_inflight_messages_before_returning() {
    // Mensagem que o handler vai processar por 300ms; shutdown é sinalizado
    // em 50ms. run() só deve retornar APÓS o handler completar.
    let batch = vec![build_message("slow", r#"{"orderId":"o-1","amount":1}"#)];

    let receive = Arc::new(MockReceiveClient::new(vec![batch]));
    let delete = Arc::new(MockDeleteClient::new());
    let broker = Arc::new(StandaloneSqsBroker::new(
        receive.clone(),
        delete.clone(),
        QUEUE_URL.to_string(),
        QUEUE_NAME.to_string(),
    ));

    let processed = Arc::new(AtomicU32::new(0));
    let processed_clone = processed.clone();
    EventRouter::new()
        .subscribe::<Order, _, _>(QUEUE_NAME, move |_o: Order| {
            let p = processed_clone.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                p.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .attach(broker.clone())
        .await
        .unwrap();

    let broker_run = broker.clone();
    let started = std::time::Instant::now();
    let handle = tokio::spawn(async move { broker_run.run().await });

    // Aguarda receive ter retornado a mensagem e o handler ter iniciado
    tokio::time::sleep(Duration::from_millis(50)).await;
    broker.signal_shutdown();

    handle.await.unwrap().unwrap();
    let elapsed = started.elapsed();

    assert_eq!(
        processed.load(Ordering::SeqCst),
        1,
        "shutdown deve aguardar handler completar (graceful drain)"
    );
    assert!(
        elapsed >= Duration::from_millis(300),
        "elapsed={elapsed:?} — deveria ser >= 300ms (handler em voo)"
    );
    assert_eq!(delete.deleted_ids(), vec!["slow".to_string()]);
}

#[tokio::test(flavor = "current_thread")]
async fn run_continues_after_receive_errors() {
    let batch = vec![build_message("after-err", r#"{"orderId":"o","amount":1}"#)];

    // Falha as primeiras 2 chamadas, depois entrega o batch.
    let receive = Arc::new(MockReceiveClient::new(vec![batch]).with_failures(2));
    let delete = Arc::new(MockDeleteClient::new());
    let cfg = StandaloneConfig {
        error_backoff: Duration::from_millis(10),
        ..Default::default()
    };
    let broker = Arc::new(
        StandaloneSqsBroker::new(
            receive.clone(),
            delete.clone(),
            QUEUE_URL.to_string(),
            QUEUE_NAME.to_string(),
        )
        .with_config(cfg),
    );

    let received = Arc::new(AtomicU32::new(0));
    let received_clone = received.clone();
    EventRouter::new()
        .subscribe::<Order, _, _>(QUEUE_NAME, move |_o: Order| {
            let r = received_clone.clone();
            async move {
                r.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .attach(broker.clone())
        .await
        .unwrap();

    let broker_run = broker.clone();
    let handle = tokio::spawn(async move { broker_run.run().await });

    for _ in 0..50 {
        if received.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    broker.signal_shutdown();
    handle.await.unwrap().unwrap();

    assert!(receive.error_count() >= 2);
    assert_eq!(received.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn broker_publish_returns_sink_only_error() {
    let receive = Arc::new(MockReceiveClient::new(vec![]));
    let delete = Arc::new(MockDeleteClient::new());
    let broker = StandaloneSqsBroker::new(
        receive,
        delete,
        QUEUE_URL.to_string(),
        QUEUE_NAME.to_string(),
    );

    let err = broker.publish("orders", b"payload").await;
    match err {
        Err(BrokerError::Publish(msg)) => {
            assert!(
                msg.contains("sink-only") || msg.contains("SqsProducer"),
                "mensagem deve apontar para SqsProducer: {msg}"
            );
        }
        other => panic!("esperava Err(Publish(..)), recebi {other:?}"),
    }
}

/// Benchmark in-memory: 1k mensagens consumidas em batches de 10 com handler
/// async barato. Verifica que o broker sustenta o throughput sem deadlock
/// nem backpressure indevida. Não roda contra ElasticMQ real (necessitaria
/// aws-sdk-sqs); valida o design do loop sob carga sustentada.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn benchmark_sustained_throughput_1k_messages() {
    let total = 1000_usize;
    let batch_size = 10_usize;
    let batches: Vec<Vec<SqsMessage>> = (0..total / batch_size)
        .map(|b| {
            (0..batch_size)
                .map(|i| {
                    let id = format!("m-{}", b * batch_size + i);
                    build_message(&id, r#"{"orderId":"x","amount":1}"#)
                })
                .collect()
        })
        .collect();

    let receive = Arc::new(MockReceiveClient::new(batches));
    let delete = Arc::new(MockDeleteClient::new());
    let cfg = StandaloneConfig {
        wait_time_seconds: 1,
        max_messages: 10,
        ..Default::default()
    };
    let broker = Arc::new(
        StandaloneSqsBroker::new(
            receive.clone(),
            delete.clone(),
            QUEUE_URL.to_string(),
            QUEUE_NAME.to_string(),
        )
        .with_config(cfg),
    );

    let processed = Arc::new(AtomicU32::new(0));
    let processed_clone = processed.clone();
    EventRouter::new()
        .subscribe::<Order, _, _>(QUEUE_NAME, move |_o: Order| {
            let p = processed_clone.clone();
            async move {
                p.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .attach(broker.clone())
        .await
        .unwrap();

    let started = std::time::Instant::now();
    let broker_run = broker.clone();
    let handle = tokio::spawn(async move { broker_run.run().await });

    // Aguarda processamento total (timeout 5s)
    for _ in 0..500 {
        if processed.load(Ordering::SeqCst) as usize >= total {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let elapsed = started.elapsed();
    broker.signal_shutdown();
    handle.await.unwrap().unwrap();

    assert_eq!(
        processed.load(Ordering::SeqCst) as usize,
        total,
        "deveria ter processado {total} mensagens em {elapsed:?}"
    );
    assert_eq!(delete.deleted_ids().len(), total);
    let throughput = total as f64 / elapsed.as_secs_f64();
    assert!(
        throughput >= 1000.0,
        "throughput sustentado >= 1k msgs/s; medido {throughput:.0} msgs/s em {elapsed:?}"
    );
}
