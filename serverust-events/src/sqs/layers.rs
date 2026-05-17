//! Tower [`Layer`]s específicos para a pipeline SQS.
//!
//! - [`RetryLayer`] — re-executa o `Service` interno em caso de erro, com
//!   contagem máxima de tentativas. Mantém a `SqsMessage` clonada entre
//!   tentativas.
//! - [`IdempotencyLayer`] — antes de chamar o serviço interno, consulta um
//!   [`serverust_telemetry::IdempotencyStore`] usando o `message_id` da
//!   `SqsMessage`. Se já existir registro, o handler é pulado (`Ok(())`).
//!   Em sucesso, persiste um registro mínimo (`response_body` vazio,
//!   `status_code = 200`). A semântica completa "InProgress + at-least-once →
//!   effectively-once" fica para o `IdempotencyLayer` definitivo de
//!   [`US-007`].
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

use std::time::{SystemTime, UNIX_EPOCH};

use aws_lambda_events::event::sqs::SqsMessage;
use serverust_telemetry::{IdempotencyRecord, IdempotencyStore};
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

/// [`Layer`] que faz lookup em um [`IdempotencyStore`] antes de invocar o
/// serviço interno, usando o `message_id` da [`SqsMessage`] como chave.
#[derive(Clone)]
pub struct IdempotencyLayer {
    store: Arc<dyn IdempotencyStore>,
}

impl IdempotencyLayer {
    /// Cria a layer a partir de um [`IdempotencyStore`] compartilhado.
    pub fn new(store: Arc<dyn IdempotencyStore>) -> Self {
        Self { store }
    }
}

impl std::fmt::Debug for IdempotencyLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdempotencyLayer").finish_non_exhaustive()
    }
}

impl<S> Layer<S> for IdempotencyLayer {
    type Service = IdempotencyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        IdempotencyService {
            inner,
            store: self.store.clone(),
        }
    }
}

/// `Service` produzido por [`IdempotencyLayer`].
#[derive(Clone)]
pub struct IdempotencyService<S> {
    inner: S,
    store: Arc<dyn IdempotencyStore>,
}

impl<S> std::fmt::Debug for IdempotencyService<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdempotencyService").finish_non_exhaustive()
    }
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
        Box::pin(async move {
            let key = req.message_id.clone().unwrap_or_default();
            if key.is_empty() {
                return inner.call(req).await;
            }

            match store.get(&key).await {
                Ok(Some(_)) => Ok(()),
                Ok(None) => {
                    let result = inner.call(req).await;
                    if result.is_ok() {
                        let now_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or_default();
                        let record = IdempotencyRecord {
                            key: key.clone(),
                            response_body: Vec::new(),
                            status_code: 200,
                            created_at_ms: now_ms,
                        };
                        let _ = store.put(record).await;
                    }
                    result
                }
                Err(e) => Err(BrokerError::Subscribe(format!("idempotency store: {e}"))),
            }
        })
    }
}
