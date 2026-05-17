//! SqsProducer com batching transparente (US-004).
//!
//! Publica mensagens SQS com acumulação assíncrona: flush automático ao
//! atingir `max_batch_size` (padrão 10) ou ao expirar `max_linger` (padrão
//! 200 ms). Retry exponencial para `SendMessageBatch` parcialmente falhado,
//! reenviando apenas as entradas `Failed[]`.
//!
//! Dois modos de encerramento:
//! - **Graceful implícito**: dropa todos os clones do producer → canal fecha
//!   → background task faz flush e sai.
//! - **Graceful explícito**: chame `signal_shutdown()` em qualquer clone →
//!   background task drena o canal, faz flush e sai imediatamente (sem esperar
//!   o linger expirar). Útil quando outros clones ainda estão vivos.
//!
//! # Uso típico
//!
//! ```rust,ignore
//! let (producer, task) = SqsProducer::new(client, queue_url, ProducerConfig::default());
//! let msg_id = producer.send("{ \"order\": 42 }").await?;
//! drop(producer);
//! task.await.unwrap();
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tracing::{error, warn};

static MSG_COUNTER: AtomicU64 = AtomicU64::new(0);
fn next_id() -> String {
    MSG_COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

// --------------------------------------------------------------------------
// Tipos públicos
// --------------------------------------------------------------------------

/// Entrada para `SendMessageBatch`.
#[derive(Debug, Clone)]
pub struct SendEntry {
    /// ID interno do batch (correlaciona entrada com resultado).
    pub id: String,
    /// Corpo da mensagem.
    pub message_body: String,
    /// Atributos opcionais da mensagem.
    pub message_attributes: HashMap<String, String>,
    /// Grupo de ordering FIFO. `Some` apenas em filas FIFO; em standard fica `None`.
    pub message_group_id: Option<String>,
    /// Identificador de deduplicação FIFO. `Some` apenas quando aplicável.
    pub message_deduplication_id: Option<String>,
}

/// Resultado de `SendMessageBatch`.
pub struct SendResult {
    /// `(batch_id, message_id atribuído pelo SQS)` por mensagem bem-sucedida.
    pub successful: Vec<(String, String)>,
    /// `batch_id`s das entradas que falharam (devem ser retentadas).
    pub failed: Vec<String>,
}

/// Trait que abstrai a chamada de rede `SendMessageBatch`.
///
/// Implemente sobre `aws-sdk-sqs` em produção. Use mocks em testes.
#[async_trait]
pub trait SendClient: Send + Sync + 'static {
    async fn send_batch(
        &self,
        queue_url: &str,
        entries: Vec<SendEntry>,
    ) -> Result<SendResult, String>;
}

/// Erros retornados por [`SqsProducer::send`].
#[derive(Debug, Clone, Error)]
pub enum SendError {
    #[error("produtor encerrado")]
    Shutdown,
    #[error("send falhou após retries: {0}")]
    RetryExhausted(String),
}

/// ID de mensagem atribuído pelo SQS após confirmação.
pub type MessageId = String;

/// Configuração do [`SqsProducer`].
#[derive(Debug, Clone)]
pub struct ProducerConfig {
    /// Tamanho máximo do batch antes do flush (padrão: 10, limite SQS).
    pub max_batch_size: usize,
    /// Tempo máximo de acumulação após a primeira mensagem do batch (padrão: 200 ms).
    pub max_linger: Duration,
    /// Número máximo de tentativas por flush (padrão: 3).
    pub max_retries: u32,
    /// Backoff base para retry exponencial (padrão: 100 ms; use ZERO em testes).
    pub base_backoff: Duration,
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 10,
            max_linger: Duration::from_millis(200),
            max_retries: 3,
            base_backoff: Duration::from_millis(100),
        }
    }
}

// --------------------------------------------------------------------------
// Internals
// --------------------------------------------------------------------------

struct PendingMessage {
    id: String,
    body: String,
    attributes: HashMap<String, String>,
    message_group_id: Option<String>,
    message_deduplication_id: Option<String>,
    tx: oneshot::Sender<Result<MessageId, SendError>>,
}

// --------------------------------------------------------------------------
// SqsProducer
// --------------------------------------------------------------------------

/// Producer SQS com batching transparente.
///
/// Clone barato — cada clone compartilha o mesmo canal de envio.
/// Quando todos os clones são dropados, o canal fecha e a task de background
/// faz flush das mensagens pendentes antes de encerrar.
///
/// Para encerrar explicitamente (sem dropar todos os clones):
/// ```rust,ignore
/// producer.signal_shutdown(); // sinaliza flush imediato
/// drop(producer);
/// task.await.unwrap();
/// ```
#[derive(Clone)]
pub struct SqsProducer {
    tx: mpsc::Sender<PendingMessage>,
    shutdown_tx: Arc<watch::Sender<bool>>,
}

impl SqsProducer {
    /// Cria um novo producer e retorna `(producer, task_handle)`.
    ///
    /// A `task_handle` encerra quando o canal fecha (todos os clones dropados)
    /// ou quando `signal_shutdown()` é chamado.
    pub fn new(
        client: Arc<dyn SendClient>,
        queue_url: impl Into<String>,
        config: ProducerConfig,
    ) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(1024);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let queue_url = queue_url.into();
        let task = tokio::spawn(producer_task(client, queue_url, rx, shutdown_rx, config));
        (
            Self {
                tx,
                shutdown_tx: Arc::new(shutdown_tx),
            },
            task,
        )
    }

    /// Sinaliza shutdown gracioso: a background task drena o canal,
    /// faz flush dos pendentes e encerra. Útil quando outros clones ainda
    /// estão vivos (ex: spawned tasks aguardando resultados).
    pub fn signal_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Envia uma mensagem e retorna o `MessageId` após confirmação do SQS.
    pub async fn send(&self, body: impl Into<String>) -> Result<MessageId, SendError> {
        self.send_with_attributes(body, HashMap::new()).await
    }

    /// Envia uma mensagem com atributos customizados.
    pub async fn send_with_attributes(
        &self,
        body: impl Into<String>,
        attributes: HashMap<String, String>,
    ) -> Result<MessageId, SendError> {
        self.enqueue(body.into(), attributes, None, None).await
    }

    /// Enfileira uma mensagem com campos FIFO opcionais.
    ///
    /// Usado pelo [`super::fifo_producer::SqsFifoProducer`] após o builder
    /// type-state ter exigido `message_group_id` em compile-time.
    pub(crate) async fn enqueue(
        &self,
        body: String,
        attributes: HashMap<String, String>,
        message_group_id: Option<String>,
        message_deduplication_id: Option<String>,
    ) -> Result<MessageId, SendError> {
        let (result_tx, result_rx) = oneshot::channel();
        let msg = PendingMessage {
            id: next_id(),
            body,
            attributes,
            message_group_id,
            message_deduplication_id,
            tx: result_tx,
        };
        self.tx.send(msg).await.map_err(|_| SendError::Shutdown)?;
        result_rx.await.unwrap_or(Err(SendError::Shutdown))
    }
}

// --------------------------------------------------------------------------
// Background task
// --------------------------------------------------------------------------

async fn producer_task(
    client: Arc<dyn SendClient>,
    queue_url: String,
    mut rx: mpsc::Receiver<PendingMessage>,
    mut shutdown_rx: watch::Receiver<bool>,
    config: ProducerConfig,
) {
    let mut pending: Vec<PendingMessage> = Vec::new();
    let mut linger_deadline: Option<tokio::time::Instant> = None;

    loop {
        let timeout = match linger_deadline {
            Some(dl) => {
                let now = tokio::time::Instant::now();
                if now >= dl { Duration::ZERO } else { dl - now }
            }
            None => Duration::from_secs(3600),
        };

        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(msg) => {
                        if pending.is_empty() {
                            linger_deadline =
                                Some(tokio::time::Instant::now() + config.max_linger);
                        }
                        pending.push(msg);
                        if pending.len() >= config.max_batch_size {
                            flush(&client, &queue_url, &mut pending, &config).await;
                            linger_deadline = None;
                        }
                    }
                    None => {
                        // Canal fechado — todos os producers foram dropados.
                        if !pending.is_empty() {
                            flush(&client, &queue_url, &mut pending, &config).await;
                        }
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(timeout) => {
                if !pending.is_empty() {
                    flush(&client, &queue_url, &mut pending, &config).await;
                }
                linger_deadline = None;
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    // Drena mensagens ainda no canal antes de encerrar.
                    while let Ok(msg) = rx.try_recv() {
                        pending.push(msg);
                    }
                    if !pending.is_empty() {
                        flush(&client, &queue_url, &mut pending, &config).await;
                    }
                    break;
                }
            }
        }
    }
}

async fn flush(
    client: &Arc<dyn SendClient>,
    queue_url: &str,
    pending: &mut Vec<PendingMessage>,
    config: &ProducerConfig,
) {
    if pending.is_empty() {
        return;
    }

    // Constrói lookup id → PendingMessage
    let batch: HashMap<String, PendingMessage> =
        pending.drain(..).map(|m| (m.id.clone(), m)).collect();

    let mut to_send: Vec<SendEntry> = batch
        .values()
        .map(|m| SendEntry {
            id: m.id.clone(),
            message_body: m.body.clone(),
            message_attributes: m.attributes.clone(),
            message_group_id: m.message_group_id.clone(),
            message_deduplication_id: m.message_deduplication_id.clone(),
        })
        .collect();

    let mut resolved: HashMap<String, Result<MessageId, SendError>> = HashMap::new();

    for attempt in 1..=config.max_retries {
        match client.send_batch(queue_url, to_send.clone()).await {
            Ok(result) => {
                for (id, msg_id) in result.successful {
                    resolved.insert(id, Ok(msg_id));
                }
                if result.failed.is_empty() {
                    break;
                }
                let failed_set: HashSet<_> = result.failed.into_iter().collect();
                to_send.retain(|e| failed_set.contains(&e.id));
                warn!(
                    attempt,
                    max_retries = config.max_retries,
                    retrying = to_send.len(),
                    "sqs producer partial failure; retentando",
                );
                if attempt < config.max_retries && !config.base_backoff.is_zero() {
                    tokio::time::sleep(config.base_backoff * 2u32.pow(attempt - 1)).await;
                }
            }
            Err(e) => {
                error!(
                    attempt,
                    max_retries = config.max_retries,
                    error = %e,
                    "sqs producer send error",
                );
                if attempt < config.max_retries && !config.base_backoff.is_zero() {
                    tokio::time::sleep(config.base_backoff * 2u32.pow(attempt - 1)).await;
                }
            }
        }
    }

    // Entradas ainda em to_send sem resultado = retries esgotados
    for entry in to_send {
        resolved
            .entry(entry.id)
            .or_insert_with(|| Err(SendError::RetryExhausted("max retries excedido".into())));
    }

    // Notifica os callers
    for (id, msg) in batch {
        let result = resolved
            .remove(&id)
            .unwrap_or(Err(SendError::RetryExhausted("resultado ausente".into())));
        let _ = msg.tx.send(result);
    }
}
