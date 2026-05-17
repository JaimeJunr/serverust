//! Testes de integração para SqsProducer (US-004).
//!
//! Cobre os critérios de aceitação:
//! - send() retorna Result<MessageId, SendError> após confirmação
//! - Acumulação assíncrona com flush por max_batch_size=10 ou max_linger
//! - Retry exponencial em SendMessageBatch parcialmente falho
//! - Graceful shutdown faz flush antes de encerrar
//! - SendError quando o client falha persistentemente após max_retries

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serverust_events::sqs::producer::{
    ProducerConfig, SendClient, SendEntry, SendError, SendResult, SqsProducer,
};

type CallLog = Arc<Mutex<Vec<Vec<SendEntry>>>>;

// --------------------------------------------------------------------------
// Mock clients
// --------------------------------------------------------------------------

/// Sempre sucede, registrando cada chamada em `calls`.
struct AlwaysOkClient {
    calls: Arc<Mutex<Vec<Vec<SendEntry>>>>,
}

impl AlwaysOkClient {
    fn new() -> (Arc<Self>, CallLog) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                calls: calls.clone(),
            }),
            calls,
        )
    }
}

#[async_trait]
impl SendClient for AlwaysOkClient {
    async fn send_batch(
        &self,
        _queue_url: &str,
        entries: Vec<SendEntry>,
    ) -> Result<SendResult, String> {
        self.calls.lock().unwrap().push(entries.clone());
        let successful = entries
            .iter()
            .map(|e| (e.id.clone(), format!("aws-msg-{}", e.id)))
            .collect();
        Ok(SendResult {
            successful,
            failed: vec![],
        })
    }
}

/// Falha a primeira entrada do primeiro batch; sucede nas chamadas seguintes.
struct FirstEntryFailOnce {
    call_count: Arc<AtomicU32>,
    calls: Arc<Mutex<Vec<Vec<SendEntry>>>>,
}

impl FirstEntryFailOnce {
    fn new() -> (Arc<Self>, CallLog) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                call_count: Arc::new(AtomicU32::new(0)),
                calls: calls.clone(),
            }),
            calls,
        )
    }
}

#[async_trait]
impl SendClient for FirstEntryFailOnce {
    async fn send_batch(
        &self,
        _queue_url: &str,
        entries: Vec<SendEntry>,
    ) -> Result<SendResult, String> {
        let n = self.call_count.fetch_add(1, Ordering::SeqCst);
        self.calls.lock().unwrap().push(entries.clone());

        if n == 0 && !entries.is_empty() {
            // Primeira chamada: falha a primeira entrada do batch
            let failed_id = entries[0].id.clone();
            let successful = entries[1..]
                .iter()
                .map(|e| (e.id.clone(), format!("aws-msg-{}", e.id)))
                .collect();
            Ok(SendResult {
                successful,
                failed: vec![failed_id],
            })
        } else {
            let successful = entries
                .iter()
                .map(|e| (e.id.clone(), format!("aws-msg-{}", e.id)))
                .collect();
            Ok(SendResult {
                successful,
                failed: vec![],
            })
        }
    }
}

/// Sempre retorna erro de rede.
struct AlwaysErrClient;

#[async_trait]
impl SendClient for AlwaysErrClient {
    async fn send_batch(
        &self,
        _queue_url: &str,
        _entries: Vec<SendEntry>,
    ) -> Result<SendResult, String> {
        Err("network error".into())
    }
}

// --------------------------------------------------------------------------
// Testes
// --------------------------------------------------------------------------

#[tokio::test]
async fn test_send_single_message_retorna_message_id() {
    let (client, _calls) = AlwaysOkClient::new();
    let (producer, task) = SqsProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123456789/orders",
        ProducerConfig {
            max_linger: Duration::from_millis(50),
            base_backoff: Duration::ZERO,
            ..Default::default()
        },
    );

    let result = producer.send("hello world").await;
    assert!(result.is_ok(), "esperava Ok(MessageId), got {result:?}");
    let msg_id = result.unwrap();
    assert!(!msg_id.is_empty(), "MessageId não deve ser vazio");

    drop(producer);
    task.await.unwrap();
}

#[tokio::test]
async fn test_10_mensagens_acumuladas_fazem_uma_unica_chamada_ao_atingir_batch_size() {
    let (client, calls) = AlwaysOkClient::new();
    let (producer, task) = SqsProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123456789/orders",
        ProducerConfig {
            max_batch_size: 10,
            max_linger: Duration::from_secs(10), // linger longo para não flush prematuro
            base_backoff: Duration::ZERO,
            ..Default::default()
        },
    );

    // Envia 10 mensagens concorrentemente
    let mut join_set = tokio::task::JoinSet::new();
    for i in 0..10usize {
        let p = producer.clone();
        join_set.spawn(async move { p.send(format!("msg-{i}")).await });
    }

    let mut results = Vec::new();
    while let Some(r) = join_set.join_next().await {
        results.push(r.expect("join ok"));
    }

    for r in &results {
        assert!(r.is_ok(), "esperava Ok, got {r:?}");
    }

    // Deve ter feito exatamente 1 chamada com 10 entradas
    {
        let call_log = calls.lock().unwrap();
        assert_eq!(
            call_log.len(),
            1,
            "esperava 1 chamada send_batch, got {}",
            call_log.len()
        );
        assert_eq!(call_log[0].len(), 10, "esperava 10 entradas no batch");
    }

    drop(producer);
    task.await.unwrap();
}

#[tokio::test]
async fn test_flush_ocorre_apos_linger_timeout_mesmo_com_batch_incompleto() {
    let (client, calls) = AlwaysOkClient::new();
    let (producer, task) = SqsProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123456789/orders",
        ProducerConfig {
            max_batch_size: 10,
            max_linger: Duration::from_millis(50),
            base_backoff: Duration::ZERO,
            ..Default::default()
        },
    );

    // Envia apenas 2 mensagens (abaixo do batch_size=10)
    let r1 = producer.send("msg-a").await;
    let r2 = producer.send("msg-b").await;

    assert!(r1.is_ok(), "r1 esperava Ok, got {r1:?}");
    assert!(r2.is_ok(), "r2 esperava Ok, got {r2:?}");

    // Deve ter flushed pelo linger timeout
    {
        let call_log = calls.lock().unwrap();
        assert!(
            !call_log.is_empty(),
            "esperava ao menos 1 chamada send_batch após linger"
        );
    }

    drop(producer);
    task.await.unwrap();
}

#[tokio::test]
async fn test_retry_parcial_reenvia_apenas_entradas_falhadas() {
    let (client, calls) = FirstEntryFailOnce::new();
    let (producer, task) = SqsProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123456789/orders",
        ProducerConfig {
            max_batch_size: 3,
            max_linger: Duration::from_millis(50),
            base_backoff: Duration::ZERO,
            max_retries: 3,
        },
    );

    let mut join_set = tokio::task::JoinSet::new();
    for i in 0..3usize {
        let p = producer.clone();
        join_set.spawn(async move { p.send(format!("msg-{i}")).await });
    }

    let mut results = Vec::new();
    while let Some(r) = join_set.join_next().await {
        results.push(r.expect("join ok"));
    }

    // Todas as 3 mensagens devem ter sucesso (a falhada foi retentada)
    for r in &results {
        assert!(r.is_ok(), "esperava Ok após retry, got {r:?}");
    }

    {
        let call_log = calls.lock().unwrap();
        assert!(
            call_log.len() >= 2,
            "esperava >= 2 chamadas (original + retry), got {}",
            call_log.len()
        );
        // O segundo batch (retry) deve conter apenas 1 entrada
        assert_eq!(
            call_log[1].len(),
            1,
            "retry deve reenviar apenas a entrada falhada"
        );
    }

    drop(producer);
    task.await.unwrap();
}

#[tokio::test]
async fn test_graceful_shutdown_faz_flush_de_pendentes_antes_de_encerrar() {
    let (client, calls) = AlwaysOkClient::new();
    let (producer, task) = SqsProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123456789/orders",
        ProducerConfig {
            max_batch_size: 100,                 // grande — não faz flush automático
            max_linger: Duration::from_secs(60), // longo — não faz flush por timeout
            base_backoff: Duration::ZERO,
            ..Default::default()
        },
    );

    // Lança 5 sends concorrentes (ficam pendentes pois batch_size=100 e linger=60s)
    let mut join_set = tokio::task::JoinSet::new();
    for i in 0..5usize {
        let p = producer.clone();
        join_set.spawn(async move { p.send(format!("msg-{i}")).await });
    }

    // Cede ao scheduler para que os spawned tasks enfileiem suas mensagens
    tokio::task::yield_now().await;

    // signal_shutdown força flush imediato mesmo com clones ainda vivos
    producer.signal_shutdown();
    drop(producer);
    task.await.unwrap();

    let mut results = Vec::new();
    while let Some(r) = join_set.join_next().await {
        results.push(r.expect("join ok"));
    }

    for r in &results {
        assert!(r.is_ok(), "esperava Ok após shutdown flush, got {r:?}");
    }

    let call_log = calls.lock().unwrap();
    assert!(
        !call_log.is_empty(),
        "shutdown deve ter flushed as mensagens pendentes"
    );
    let total_sent: usize = call_log.iter().map(|b| b.len()).sum();
    assert_eq!(
        total_sent, 5,
        "shutdown deve ter enviado todas as 5 mensagens"
    );
}

#[tokio::test]
async fn test_send_retorna_erro_apos_max_retries_esgotado() {
    let client = Arc::new(AlwaysErrClient);
    let (producer, task) = SqsProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123456789/orders",
        ProducerConfig {
            max_batch_size: 1,
            max_linger: Duration::from_millis(50),
            base_backoff: Duration::ZERO,
            max_retries: 2,
        },
    );

    let result = producer.send("msg").await;
    assert!(
        matches!(result, Err(SendError::RetryExhausted(_))),
        "esperava RetryExhausted, got {result:?}"
    );

    drop(producer);
    task.await.unwrap();
}

/// Benchmark ignorado por padrão — requer ElasticMQ em ELASTICMQ_URL.
/// Rode com: cargo test -p serverust-events --test sqs_producer -- --ignored
#[tokio::test]
#[ignore]
async fn benchmark_10k_msgs_em_1_2s() {
    let _url = match std::env::var("ELASTICMQ_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("ELASTICMQ_URL não definida; benchmark ignorado");
            return;
        }
    };
    // Implementação completa com aws-sdk-sqs client real em US-010.
    eprintln!("Benchmark ElasticMQ — a implementar com aws-sdk-sqs em US-010");
}
