//! Tower [`Layer`]s específicos para a pipeline SQS.
//!
//! - [`RetryLayer`] — re-executa o `Service` interno em caso de erro, com
//!   contagem máxima de tentativas. Mantém a `SqsMessage` clonada entre
//!   tentativas.
//! - [`IdempotencyLayer`] — protocolo InProgress/Completed atrás de
//!   [`serverust_telemetry::IdempotencyStore`]. Antes de chamar o handler,
//!   tenta adquirir lock (`try_acquire`) com TTL configurável. Se o lock
//!   estiver `Completed` dentro do TTL, skipa com `Ok(())`. Se estiver
//!   `InProgress` (outro worker possui o lock), retorna erro para que o SQS
//!   coloque a mensagem em visibility timeout. Se a chave for nova ou o
//!   registro tiver expirado, processa e grava o resultado.
//!
//! Ambos os layers preservam o contrato `Service<SqsMessage, Response = (),
//! Error = BrokerError>` para encadear com [`super::subscriber::SqsSubscriber`]
//! e com [`serverust_telemetry::tower::TracingLayer`].
//!
//! # Ordem default da pipeline
//!
//! ```text
//! inbound → TracingLayer → IdempotencyLayer → RetryLayer → handler
//! ```
//!
//! Tracing é o layer mais externo (registra a entrada antes de qualquer
//! decisão de idempotência ou retry). Idempotência fica entre tracing e retry
//! para que tentativas repetidas pelo `RetryLayer` não atravessem o
//! `IdempotencyStore` novamente (a chave já está em `InProgress` no primeiro
//! call). Retry é o layer mais interno, perto do handler.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use std::time::{SystemTime, UNIX_EPOCH};

use aws_lambda_events::event::sqs::SqsMessage;
use serverust_telemetry::IdempotencyStore;
use serverust_telemetry::idempotency::AcquireOutcome;
use tower::{Layer, Service};

use crate::broker::BrokerError;

// ---------- RetryLayer ----------

/// [`Layer`] que reexecuta o `Service` interno até `max_attempts` vezes em
/// caso de erro.
#[derive(Clone, Debug)]
pub struct RetryLayer {
    max_attempts: u32,
}

impl RetryLayer {
    /// Cria a layer com o número máximo de tentativas (`>= 1`).
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
        }
    }
}

impl<S> Layer<S> for RetryLayer {
    type Service = RetryService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RetryService {
            inner,
            max_attempts: self.max_attempts,
        }
    }
}

/// `Service` produzido por [`RetryLayer`].
#[derive(Clone, Debug)]
pub struct RetryService<S> {
    inner: S,
    max_attempts: u32,
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
        Box::pin(async move {
            let mut last_err: Option<BrokerError> = None;
            for _ in 0..max_attempts {
                match inner.call(req.clone()).await {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
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
