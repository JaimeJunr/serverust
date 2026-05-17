//! Heartbeat de visibility timeout automático para handlers SQS longos (US-009).
//!
//! [`HeartbeatLayer`] envolve um `Service<SqsMessage>` e, em background,
//! chama `ChangeMessageVisibility` antes que o visibility timeout expire —
//! evitando que mensagens em processamento voltem para a fila e sejam
//! reprocessadas.
//!
//! # Funcionamento
//!
//! Para cada mensagem processada, o layer:
//!
//! 1. Spawna uma task em background que dorme por
//!    `visibility_timeout × (1 - threshold/100)` e então chama
//!    [`HeartbeatClient::change_visibility`] para estender o timeout. O ciclo
//!    se repete até a task ser cancelada.
//! 2. Executa o handler normalmente.
//! 3. Ao terminar (Ok ou Err), aborta a task de heartbeat.
//!
//! ## Default
//!
//! `threshold = 30` — heartbeat disparado quando restam 30% do timeout
//! (i.e., após 70% decorrido).
//!
//! ## Modo de uso
//!
//! - **Lambda ESM**: opt-in — adicione o layer explicitamente na pipeline.
//! - **StandaloneSqsBroker** (US-010): ativo por default.
//!
//! ## Mensagem sem `receipt_handle`
//!
//! Se a [`SqsMessage`] não tiver `receipt_handle`, o heartbeat é silenciosamente
//! desabilitado para aquela mensagem. O handler ainda é invocado normalmente.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use aws_lambda_events::event::sqs::SqsMessage;
use tower::{Layer, Service};

use crate::broker::BrokerError;

/// Threshold padrão: 30% do timeout restante.
const DEFAULT_THRESHOLD_PCT: u8 = 30;

/// Cliente abstrato para `ChangeMessageVisibility`.
///
/// Implementações reais delegam para `aws_sdk_sqs::Client::change_message_visibility`.
/// Em testes, use um mock que registra as chamadas.
#[async_trait]
pub trait HeartbeatClient: Send + Sync + 'static {
    /// Estende o visibility timeout de uma mensagem.
    ///
    /// - `queue_url`: URL da fila SQS.
    /// - `receipt_handle`: handle de recibo da mensagem (campo [`SqsMessage::receipt_handle`]).
    /// - `visibility_timeout_secs`: novo timeout em segundos.
    async fn change_visibility(
        &self,
        queue_url: &str,
        receipt_handle: &str,
        visibility_timeout_secs: i32,
    ) -> Result<(), String>;
}

/// [`Layer`] que mantém o visibility timeout de mensagens longas renovado
/// automaticamente em background.
///
/// Ver documentação do módulo para detalhes de funcionamento.
#[derive(Clone)]
pub struct HeartbeatLayer {
    client: Arc<dyn HeartbeatClient>,
    queue_url: Arc<str>,
    visibility_timeout: Duration,
    threshold_pct: u8,
}

impl HeartbeatLayer {
    /// Cria o layer.
    ///
    /// - `client`: implementação de [`HeartbeatClient`].
    /// - `queue_url`: URL da fila SQS (usada na chamada `ChangeMessageVisibility`).
    /// - `visibility_timeout`: timeout configurado na fila (usado para calcular
    ///   o intervalo do heartbeat).
    pub fn new(
        client: Arc<dyn HeartbeatClient>,
        queue_url: impl Into<String>,
        visibility_timeout: Duration,
    ) -> Self {
        Self {
            client,
            queue_url: Arc::from(queue_url.into()),
            visibility_timeout,
            threshold_pct: DEFAULT_THRESHOLD_PCT,
        }
    }

    /// Configura o threshold do heartbeat.
    ///
    /// `pct` é a porcentagem **restante** do visibility timeout quando o
    /// heartbeat é disparado. Exemplo: `with_threshold(30)` (default) dispara
    /// quando restar 30% do timeout (após 70% decorrido). Valores > 99 são
    /// saturados em 99.
    pub fn with_threshold(mut self, pct: u8) -> Self {
        self.threshold_pct = pct.min(99);
        self
    }
}

impl std::fmt::Debug for HeartbeatLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeartbeatLayer")
            .field("queue_url", &self.queue_url)
            .field("visibility_timeout", &self.visibility_timeout)
            .field("threshold_pct", &self.threshold_pct)
            .finish_non_exhaustive()
    }
}

impl<S> Layer<S> for HeartbeatLayer {
    type Service = HeartbeatService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HeartbeatService {
            inner,
            client: self.client.clone(),
            queue_url: self.queue_url.clone(),
            visibility_timeout: self.visibility_timeout,
            threshold_pct: self.threshold_pct,
        }
    }
}

/// `Service` produzido por [`HeartbeatLayer`].
#[derive(Clone)]
pub struct HeartbeatService<S> {
    inner: S,
    client: Arc<dyn HeartbeatClient>,
    queue_url: Arc<str>,
    visibility_timeout: Duration,
    threshold_pct: u8,
}

impl<S> std::fmt::Debug for HeartbeatService<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeartbeatService")
            .field("queue_url", &self.queue_url)
            .field("visibility_timeout", &self.visibility_timeout)
            .field("threshold_pct", &self.threshold_pct)
            .finish_non_exhaustive()
    }
}

impl<S> Service<SqsMessage> for HeartbeatService<S>
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
        let queue_url = self.queue_url.clone();
        let visibility_timeout = self.visibility_timeout;
        let threshold_pct = self.threshold_pct;

        Box::pin(async move {
            let receipt_handle = match req.receipt_handle.clone() {
                Some(rh) if !rh.is_empty() => rh,
                _ => {
                    // Sem receipt_handle: processa normalmente, sem heartbeat.
                    return inner.call(req).await;
                }
            };

            // Intervalo antes do primeiro heartbeat: `visibility_timeout * (1 - threshold/100)`.
            let elapsed_fraction = 1.0 - (threshold_pct as f64 / 100.0);
            let heartbeat_interval = duration_fraction(visibility_timeout, elapsed_fraction);
            let vt_secs = visibility_timeout.as_secs().max(1) as i32;

            // Spawna task de heartbeat; será abortada após o handler completar.
            let heartbeat_task = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(heartbeat_interval).await;
                    if client
                        .change_visibility(&queue_url, &receipt_handle, vt_secs)
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });

            let result = inner.call(req).await;
            heartbeat_task.abort();

            result
        })
    }
}

/// Calcula `duration * fraction` com precisão de ponto flutuante.
fn duration_fraction(d: Duration, fraction: f64) -> Duration {
    let millis = (d.as_millis() as f64 * fraction) as u64;
    Duration::from_millis(millis.max(1))
}
