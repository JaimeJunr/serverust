//! Tower [`Layer`]s específicos para a pipeline SQS.
//!
//! - [`RetryLayer`] — re-executa o `Service` interno em caso de erro, com
//!   contagem máxima de tentativas e backoff exponencial opcional (cap pelo
//!   visibility timeout via [`RetryLayer::with_backoff`]).
//! - [`IdempotencyLayer`] — protocolo InProgress/Completed atrás de
//!   [`serverust_telemetry::IdempotencyStore`]. Antes de chamar o handler,
//!   tenta adquirir lock (`try_acquire`) com TTL configurável. Se o lock
//!   estiver `Completed` dentro do TTL, skipa com `Ok(())`. Se estiver
//!   `InProgress` (outro worker possui o lock), retorna erro para que o SQS
//!   coloque a mensagem em visibility timeout. Se a chave for nova ou o
//!   registro tiver expirado, processa e grava o resultado.
//! - [`DlqLayer`] — em caso de erro do inner service, roteia a mensagem para
//!   uma fila DLQ via [`DlqClient::send_to_dlq`] adicionando
//!   [`FAILURE_REASON_ATTR`] (`_serverust_failure_reason`) com o motivo, e
//!   emite uma métrica [`DlqMetric`] (`serverust.sqs.dlq_routed`) por
//!   queue/handler. Se o DLQ aceitar a mensagem, o erro é absorvido (retorna
//!   `Ok(())`) para que o SQS apague o item original; se o DLQ falhar, o erro
//!   é propagado.
//!
//! Todos os layers preservam o contrato `Service<SqsMessage, Response = (),
//! Error = BrokerError>` para encadear com [`super::subscriber::SqsSubscriber`]
//! e com [`serverust_telemetry::tower::TracingLayer`].
//!
//! # Ordem default da pipeline
//!
//! ```text
//! inbound → TracingLayer → IdempotencyLayer → DlqLayer → RetryLayer → handler
//! ```
//!
//! Tracing é o layer mais externo (registra a entrada antes de qualquer
//! decisão de idempotência ou retry). Idempotência fica logo abaixo para que
//! tentativas repetidas pelo `RetryLayer` não atravessem o `IdempotencyStore`
//! novamente (a chave já está em `InProgress` no primeiro call). Retry é o
//! layer mais interno, e DlqLayer captura a falha final após esgotar os
//! retries.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use aws_lambda_events::event::sqs::SqsMessage;
use serverust_telemetry::IdempotencyStore;
use tracing::warn;
use serverust_telemetry::idempotency::AcquireOutcome;
use tower::{Layer, Service};

use crate::broker::BrokerError;

// ---------- RetryLayer ----------

/// [`Layer`] que reexecuta o `Service` interno até `max_attempts` vezes em
/// caso de erro. Pode aplicar backoff exponencial entre tentativas via
/// [`RetryLayer::with_backoff`].
#[derive(Clone, Debug)]
pub struct RetryLayer {
    max_attempts: u32,
    backoff_base: Duration,
    backoff_max_total: Duration,
}

impl RetryLayer {
    /// Cria a layer com o número máximo de tentativas (`>= 1`).
    /// Sem backoff por padrão (retries imediatos).
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            backoff_base: Duration::ZERO,
            backoff_max_total: Duration::ZERO,
        }
    }

    /// Habilita backoff exponencial entre tentativas.
    ///
    /// - `base`: espera inicial. A i-ésima retry (0-indexed) espera
    ///   `base * 2^i`.
    /// - `max_total`: teto absoluto para o tempo total acumulado em
    ///   backoffs. Representa o VisibilityTimeout disponível: o layer não
    ///   espera mais que esse valor entre todas as retries somadas, evitando
    ///   estender o timeout da fila.
    pub fn with_backoff(mut self, base: Duration, max_total: Duration) -> Self {
        self.backoff_base = base;
        self.backoff_max_total = max_total;
        self
    }
}

impl<S> Layer<S> for RetryLayer {
    type Service = RetryService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RetryService {
            inner,
            max_attempts: self.max_attempts,
            backoff_base: self.backoff_base,
            backoff_max_total: self.backoff_max_total,
        }
    }
}

/// `Service` produzido por [`RetryLayer`].
#[derive(Clone, Debug)]
pub struct RetryService<S> {
    inner: S,
    max_attempts: u32,
    backoff_base: Duration,
    backoff_max_total: Duration,
}

impl<S> Service<SqsMessage> for RetryService<S>
where
    S: Service<SqsMessage, Response = (), Error = BrokerError> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = ();
    type Error = BrokerError;
    type Future = Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: SqsMessage) -> Self::Future {
        let mut inner = self.inner.clone();
        let max_attempts = self.max_attempts;
        let base = self.backoff_base;
        let max_total = self.backoff_max_total;
        Box::pin(async move {
            let mut last_err: Option<BrokerError> = None;
            let mut spent = Duration::ZERO;
            for attempt in 0..max_attempts {
                match inner.call(req.clone()).await {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
                // Não espera após a última tentativa.
                let is_last = attempt + 1 == max_attempts;
                if is_last || base.is_zero() {
                    continue;
                }
                let wanted = base.saturating_mul(2u32.saturating_pow(attempt));
                let remaining = max_total.saturating_sub(spent);
                let sleep_for = wanted.min(remaining);
                if sleep_for.is_zero() {
                    continue;
                }
                tokio::time::sleep(sleep_for).await;
                spent += sleep_for;
            }
            Err(last_err.unwrap_or_else(|| BrokerError::Subscribe("retry exhausted".into())))
        })
    }
}

// ---------- IdempotencyLayer ----------

/// TTL default do lock de idempotência: 24 h (alinhado ao PRD US-007).
const DEFAULT_IDEMPOTENCY_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// [`Layer`] que faz lock InProgress→Completed em um [`IdempotencyStore`]
/// antes de invocar o serviço interno, usando o `message_id` da
/// [`SqsMessage`] como chave.
#[derive(Clone)]
pub struct IdempotencyLayer {
    store: Arc<dyn IdempotencyStore>,
    ttl: Duration,
}

impl IdempotencyLayer {
    /// Cria a layer a partir de um [`IdempotencyStore`] compartilhado.
    /// TTL default: 24 h.
    pub fn new(store: Arc<dyn IdempotencyStore>) -> Self {
        Self {
            store,
            ttl: DEFAULT_IDEMPOTENCY_TTL,
        }
    }

    /// Configura o TTL do lock (estados InProgress e Completed).
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }
}

impl std::fmt::Debug for IdempotencyLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdempotencyLayer")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl<S> Layer<S> for IdempotencyLayer {
    type Service = IdempotencyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        IdempotencyService {
            inner,
            store: self.store.clone(),
            ttl: self.ttl,
        }
    }
}

/// `Service` produzido por [`IdempotencyLayer`].
#[derive(Clone)]
pub struct IdempotencyService<S> {
    inner: S,
    store: Arc<dyn IdempotencyStore>,
    ttl: Duration,
}

impl<S> std::fmt::Debug for IdempotencyService<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdempotencyService")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

impl<S> Service<SqsMessage> for IdempotencyService<S>
where
    S: Service<SqsMessage, Response = (), Error = BrokerError> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = ();
    type Error = BrokerError;
    type Future = Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: SqsMessage) -> Self::Future {
        let mut inner = self.inner.clone();
        let store = self.store.clone();
        let ttl_ms = self.ttl.as_millis() as u64;
        Box::pin(async move {
            let key = req.message_id.clone().unwrap_or_default();
            if key.is_empty() {
                // Bypass de idempotência intencional: sem chave estável, qualquer
                // tentativa de dedupe produziria falsos positivos. Sinaliza a
                // configuração inesperada para o operador detectar fonte sem ID.
                warn!(
                    "idempotency bypass: SqsMessage sem message_id; handler executará sem dedupe",
                );
                return inner.call(req).await;
            }

            let acquired_at = now_ms();
            match store.try_acquire(&key, acquired_at, ttl_ms).await {
                Ok(AcquireOutcome::Acquired) => {
                    let result = inner.call(req).await;
                    if result.is_ok() {
                        let _ = store.complete(&key, now_ms(), ttl_ms).await;
                    }
                    result
                }
                Ok(AcquireOutcome::AlreadyCompleted(_)) => Ok(()),
                Ok(AcquireOutcome::InProgress) => Err(BrokerError::Subscribe(format!(
                    "idempotency key {key} already in progress by another worker"
                ))),
                Err(e) => Err(BrokerError::Subscribe(format!("idempotency store: {e}"))),
            }
        })
    }
}

// ---------- DlqLayer ----------

/// Nome do atributo de mensagem usado pelo [`DlqLayer`] para carregar o
/// motivo da falha. Lido pelo handler de DLQ ou pelo operador inspecionando
/// mensagens órfãs.
pub const FAILURE_REASON_ATTR: &str = "_serverust_failure_reason";

/// Nome canônico da métrica EMF emitida pelo [`DlqLayer`] (família
/// `serverust.sqs.*`). Exposto como constante para ser referenciado por
/// outros componentes (ex.: [`super::observability`]) sem duplicar o
/// literal.
pub const DLQ_ROUTED_METRIC: &str = "serverust.sqs.dlq_routed";

/// Cliente abstrato que envia uma mensagem para uma fila DLQ.
///
/// Implementações reais delegam para `aws_sdk_sqs::Client::send_message`. Em
/// testes, use um mock que armazena as chamadas.
#[async_trait]
pub trait DlqClient: Send + Sync + 'static {
    /// Envia `body` (e atributos `String → String`) para `queue`. Retorna
    /// `Err(motivo)` se o envio falhar — nesse caso o [`DlqLayer`] propaga
    /// o erro upstream para que o SQS volte a mensagem para a visibility.
    async fn send_to_dlq(
        &self,
        queue: &str,
        body: &str,
        attributes: HashMap<String, String>,
    ) -> Result<(), String>;
}

/// Métrica emitida pelo [`DlqLayer`] sempre que rotear uma mensagem para o DLQ.
///
/// Carrega o nome (`serverust.sqs.dlq_routed`), o `queue` e o `handler`
/// (ambos dimensões), e `value = 1.0` (`Count`). O sink padrão delega para
/// [`serverust_telemetry::emit_emf`]; testes podem instalar um recorder via
/// [`DlqLayer::with_metric_recorder`].
#[derive(Debug, Clone)]
pub struct DlqMetric {
    pub metric_name: &'static str,
    pub queue: String,
    pub handler: String,
    pub value: f64,
}

type MetricRecorder = Arc<dyn Fn(DlqMetric) + Send + Sync>;

/// [`Layer`] que captura erro do inner service e roteia para uma fila DLQ.
#[derive(Clone)]
pub struct DlqLayer {
    client: Arc<dyn DlqClient>,
    dlq_queue: Arc<str>,
    handler_name: Arc<str>,
    recorder: Option<MetricRecorder>,
}

impl DlqLayer {
    /// Cria a layer com o cliente DLQ, o nome da fila e o nome do handler
    /// (usado como dimensão na métrica `serverust.sqs.dlq_routed`).
    pub fn new(
        client: Arc<dyn DlqClient>,
        dlq_queue: impl Into<String>,
        handler_name: impl Into<String>,
    ) -> Self {
        Self {
            client,
            dlq_queue: Arc::from(dlq_queue.into()),
            handler_name: Arc::from(handler_name.into()),
            recorder: None,
        }
    }

    /// Substitui o sink default de métrica (que emite EMF em stdout) por uma
    /// closure customizada. Útil em testes para capturar emissões.
    pub fn with_metric_recorder<F>(mut self, recorder: F) -> Self
    where
        F: Fn(DlqMetric) + Send + Sync + 'static,
    {
        self.recorder = Some(Arc::new(recorder));
        self
    }
}

impl std::fmt::Debug for DlqLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DlqLayer")
            .field("dlq_queue", &self.dlq_queue)
            .field("handler_name", &self.handler_name)
            .finish_non_exhaustive()
    }
}

impl<S> Layer<S> for DlqLayer {
    type Service = DlqService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DlqService {
            inner,
            client: self.client.clone(),
            dlq_queue: self.dlq_queue.clone(),
            handler_name: self.handler_name.clone(),
            recorder: self.recorder.clone(),
        }
    }
}

/// `Service` produzido por [`DlqLayer`].
#[derive(Clone)]
pub struct DlqService<S> {
    inner: S,
    client: Arc<dyn DlqClient>,
    dlq_queue: Arc<str>,
    handler_name: Arc<str>,
    recorder: Option<MetricRecorder>,
}

impl<S> std::fmt::Debug for DlqService<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DlqService")
            .field("dlq_queue", &self.dlq_queue)
            .field("handler_name", &self.handler_name)
            .finish_non_exhaustive()
    }
}

impl<S> Service<SqsMessage> for DlqService<S>
where
    S: Service<SqsMessage, Response = (), Error = BrokerError> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = ();
    type Error = BrokerError;
    type Future = Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: SqsMessage) -> Self::Future {
        let mut inner = self.inner.clone();
        let client = self.client.clone();
        let dlq_queue = self.dlq_queue.clone();
        let handler_name = self.handler_name.clone();
        let recorder = self.recorder.clone();
        let body = req.body.clone().unwrap_or_default();
        Box::pin(async move {
            match inner.call(req).await {
                Ok(()) => Ok(()),
                Err(err) => {
                    let mut attrs: HashMap<String, String> = HashMap::new();
                    attrs.insert(FAILURE_REASON_ATTR.to_string(), err.to_string());
                    match client.send_to_dlq(&dlq_queue, &body, attrs).await {
                        Ok(()) => {
                            emit_dlq_metric(
                                recorder.as_deref(),
                                dlq_queue.as_ref(),
                                handler_name.as_ref(),
                            );
                            Ok(())
                        }
                        Err(dlq_err) => Err(BrokerError::Subscribe(format!(
                            "dlq routing failed for {dlq_queue}: {dlq_err}"
                        ))),
                    }
                }
            }
        })
    }
}

fn emit_dlq_metric(
    recorder: Option<&(dyn Fn(DlqMetric) + Send + Sync)>,
    queue: &str,
    handler: &str,
) {
    let metric = DlqMetric {
        metric_name: DLQ_ROUTED_METRIC,
        queue: queue.to_string(),
        handler: handler.to_string(),
        value: 1.0,
    };
    match recorder {
        Some(r) => r(metric),
        None => {
            // Sink default: EMF em stdout (ingerido pelo CloudWatch).
            // O namespace `serverust.sqs` carrega o domínio; queue/handler
            // viajam no payload da linha para inspeção operacional.
            serverust_telemetry::emit_emf(
                "serverust.sqs",
                metric.metric_name,
                "Count",
                metric.value,
            );
        }
    }
}
