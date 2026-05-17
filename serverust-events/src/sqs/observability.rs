//! `ObservabilityLayer` — métricas EMF e segmentos X-Ray automáticos para
//! subscribers SQS (US-012).
//!
//! Atrás de `feature = "sqs"`. Emite quatro métricas no namespace
//! `serverust.sqs`:
//!
//! - `serverust.sqs.messages` (Count) — uma vez por mensagem despachada.
//! - `serverust.sqs.errors` (Count) — uma vez por mensagem que terminou em
//!   `Err` no inner service.
//! - `serverust.sqs.latency` (Milliseconds) — tempo total do inner `call`.
//! - `serverust.sqs.dlq_routed` (Count) — emitida pela [`super::layers::DlqLayer`]
//!   quando uma mensagem é roteada para a DLQ (vide US-008). Mantemos a
//!   constante [`super::layers::DLQ_ROUTED_METRIC`] como ponto canônico.
//!
//! Cada chamada também abre um `tracing::info_span!("sqs.message", ...)` com
//! o `trace_id` no formato AWS X-Ray. Se a [`SqsMessage`] traz o atributo
//! de sistema `AWSTraceHeader` (formato `Root=...;Parent=...;Sampled=...`),
//! o campo `Root` é usado; caso contrário um trace id novo é gerado por
//! [`serverust_telemetry::generate_xray_compatible_trace_id`].
//!
//! O sink default das métricas chama [`serverust_telemetry::emit_emf`]
//! (linha JSON em stdout — ingerida automaticamente pelo CloudWatch Logs).
//! Em testes, instale um recorder via [`ObservabilityLayer::with_metric_recorder`].
//!
//! # Composição com `TracingLayer`
//!
//! `ObservabilityLayer` é ortogonal à [`serverust_telemetry::tower::TracingLayer`]:
//! pode-se aplicar ambos. A ordem recomendada é:
//!
//! ```text
//! inbound → TracingLayer → ObservabilityLayer → IdempotencyLayer → DlqLayer → RetryLayer → handler
//! ```
//!
//! Com `TracingLayer` mais externa, ela abre o span genérico do framework;
//! `ObservabilityLayer` abre um span filho com semântica SQS (queue, handler,
//! trace_id), mede a latência e emite as métricas.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use aws_lambda_events::event::sqs::SqsMessage;
use tower::{Layer, Service};
use tracing::Instrument;

use crate::broker::BrokerError;

/// Namespace EMF compartilhado por toda a família de métricas SQS.
pub const EMF_NAMESPACE: &str = "serverust.sqs";

/// Métrica `serverust.sqs.messages` (Count) — incrementada para cada mensagem
/// despachada.
pub const METRIC_MESSAGES: &str = "serverust.sqs.messages";

/// Métrica `serverust.sqs.errors` (Count) — incrementada quando o inner
/// service retorna `Err`.
pub const METRIC_ERRORS: &str = "serverust.sqs.errors";

/// Métrica `serverust.sqs.latency` (Milliseconds) — tempo total do inner
/// `call`.
pub const METRIC_LATENCY: &str = "serverust.sqs.latency";

/// Nome do atributo de sistema usado pelo SQS para carregar o trace context
/// do AWS X-Ray. Formato: `Root=1-...-...;Parent=...;Sampled=1`.
pub const AWS_TRACE_HEADER_ATTR: &str = "AWSTraceHeader";

/// Métrica emitida pelo [`ObservabilityLayer`]. Carrega o nome canônico
/// (`serverust.sqs.messages|.errors|.latency`), a unidade EMF e as dimensões
/// `queue` + `handler`.
#[derive(Debug, Clone)]
pub struct ObservabilityMetric {
    pub name: &'static str,
    pub unit: &'static str,
    pub value: f64,
    pub queue: String,
    pub handler: String,
}

type MetricRecorder = Arc<dyn Fn(ObservabilityMetric) + Send + Sync>;

/// [`Layer`] que envolve o `Service` interno, abre um span tracing com
/// trace_id X-Ray, mede a latência e emite as métricas EMF.
#[derive(Clone)]
pub struct ObservabilityLayer {
    handler_name: Arc<str>,
    recorder: Option<MetricRecorder>,
}

impl ObservabilityLayer {
    /// Cria a layer associada a um `handler_name` (usado como dimensão da
    /// métrica e como campo do span).
    pub fn new(handler_name: impl Into<String>) -> Self {
        Self {
            handler_name: Arc::from(handler_name.into()),
            recorder: None,
        }
    }

    /// Substitui o sink default (EMF em stdout) por um recorder customizado.
    /// Útil para captura em testes.
    pub fn with_metric_recorder<F>(mut self, recorder: F) -> Self
    where
        F: Fn(ObservabilityMetric) + Send + Sync + 'static,
    {
        self.recorder = Some(Arc::new(recorder));
        self
    }
}

impl std::fmt::Debug for ObservabilityLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObservabilityLayer")
            .field("handler_name", &self.handler_name)
            .finish_non_exhaustive()
    }
}

impl<S> Layer<S> for ObservabilityLayer {
    type Service = ObservabilityService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ObservabilityService {
            inner,
            handler_name: self.handler_name.clone(),
            recorder: self.recorder.clone(),
        }
    }
}

/// `Service` produzido por [`ObservabilityLayer`].
#[derive(Clone)]
pub struct ObservabilityService<S> {
    inner: S,
    handler_name: Arc<str>,
    recorder: Option<MetricRecorder>,
}

impl<S> std::fmt::Debug for ObservabilityService<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObservabilityService")
            .field("handler_name", &self.handler_name)
            .finish_non_exhaustive()
    }
}

impl<S> Service<SqsMessage> for ObservabilityService<S>
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
        let queue = extract_queue_name(&req).unwrap_or_else(|| "unknown".to_string());
        let trace_id = extract_or_generate_xray_trace_id(&req);
        let message_id = req.message_id.clone().unwrap_or_default();
        let handler_name = self.handler_name.clone();
        let recorder = self.recorder.clone();
        let mut inner = self.inner.clone();

        let span = tracing::info_span!(
            "sqs.message",
            otel.name = "sqs.message",
            queue = %queue,
            handler = %handler_name,
            message_id = %message_id,
            trace_id = %trace_id,
        );

        // Métrica "messages" emitida imediatamente no entrypoint do layer.
        emit_metric(
            recorder.as_deref(),
            ObservabilityMetric {
                name: METRIC_MESSAGES,
                unit: "Count",
                value: 1.0,
                queue: queue.clone(),
                handler: handler_name.to_string(),
            },
        );

        Box::pin(
            async move {
                let started = Instant::now();
                let result = inner.call(req).await;
                let latency_ms = started.elapsed().as_secs_f64() * 1_000.0;

                emit_metric(
                    recorder.as_deref(),
                    ObservabilityMetric {
                        name: METRIC_LATENCY,
                        unit: "Milliseconds",
                        value: latency_ms,
                        queue: queue.clone(),
                        handler: handler_name.to_string(),
                    },
                );

                if result.is_err() {
                    emit_metric(
                        recorder.as_deref(),
                        ObservabilityMetric {
                            name: METRIC_ERRORS,
                            unit: "Count",
                            value: 1.0,
                            queue: queue.clone(),
                            handler: handler_name.to_string(),
                        },
                    );
                }

                result
            }
            .instrument(span),
        )
    }
}

/// Extrai o trace id X-Ray do atributo de sistema `AWSTraceHeader` da
/// [`SqsMessage`] ou gera um novo no formato AWS X-Ray.
///
/// O header tem o formato `Root=1-...-...;Parent=...;Sampled=...`. Quando o
/// campo `Root=` está presente, devolvemos seu valor. Caso contrário, se o
/// header existe mas não tem `Root=`, devolvemos o header como está
/// (compatibilidade com clientes que mandam só o id). Sem header algum,
/// geramos um id novo via [`serverust_telemetry::generate_xray_compatible_trace_id`].
pub fn extract_or_generate_xray_trace_id(msg: &SqsMessage) -> String {
    if let Some(header) = msg.attributes.get(AWS_TRACE_HEADER_ATTR) {
        for segment in header.split(';') {
            if let Some(root) = segment.trim().strip_prefix("Root=") {
                return root.to_string();
            }
        }
        return header.clone();
    }
    serverust_telemetry::generate_xray_compatible_trace_id()
}

/// Extrai o nome da fila do `event_source_arn` da [`SqsMessage`] (segmento
/// final). Mantemos a função privada para reusar a lógica de
/// `consumer::extract_queue_name` sem expor o detalhe.
fn extract_queue_name(msg: &SqsMessage) -> Option<String> {
    let arn = msg.event_source_arn.as_deref()?;
    arn.rsplit(':').next().map(|s| s.to_string())
}

fn emit_metric(
    recorder: Option<&(dyn Fn(ObservabilityMetric) + Send + Sync)>,
    metric: ObservabilityMetric,
) {
    match recorder {
        Some(r) => r(metric),
        None => {
            serverust_telemetry::emit_emf(EMF_NAMESPACE, metric.name, metric.unit, metric.value);
        }
    }
}
