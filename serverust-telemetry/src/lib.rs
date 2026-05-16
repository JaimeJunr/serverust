//! Telemetria nativa do **serverust**: logger JSON estruturado, propagação de
//! correlation id compatível com AWS X-Ray, métricas em formato EMF e trait
//! de idempotência.
//!
//! Reimplementa em Rust os pilares do AWS Lambda Powertools (que não tem
//! versão oficial Rust). Foi desenhado para funcionar tanto em Lambda quanto
//! em servidor HTTP local sem mudança de código.
//!
//! # Quick start
//!
//! Chame [`init`] uma vez no boot e pronto — logs JSON estruturados e tracing
//! pré-configurados:
//!
//! ```no_run
//! use serverust_telemetry::logger;
//!
//! fn main() {
//!     logger::init();
//!     tracing::info!(user_id = 42, "request received");
//! }
//! ```
//!
//! Para propagar correlation id em handlers axum:
//!
//! ```ignore
//! use serverust_telemetry::correlation::correlation_id_middleware;
//!
//! let router = axum::Router::new()
//!     .route("/", axum::routing::get(|| async { "ok" }))
//!     .layer(axum::middleware::from_fn(correlation_id_middleware));
//! ```
//!
//! # Features opcionais
//!
//! - `otel` — adiciona `otel::init_xray` (OpenTelemetry SDK + propagador
//!   AWS X-Ray). Útil quando o tracing precisa cruzar múltiplos serviços.
//! - `dynamodb` — habilita `idempotency::DynamoDbIdempotencyStore`.
//!   Sem essa feature, [`IdempotencyStore`] continua disponível como trait
//!   (você implementa onde quiser — Redis, Postgres, memory) e
//!   [`InMemoryIdempotencyStore`] fica disponível para testes/dev.
//!
//! Sem essas features, o binário Lambda continua enxuto (< 5 MB stripped).

pub mod correlation;
pub mod dynamo;
pub mod emf;
pub mod idempotency;
pub mod logger;

#[cfg(feature = "otel")]
pub mod otel;

pub use correlation::{
    CORRELATION_ID_HEADER, X_AMZN_TRACE_ID, correlation_id_middleware,
    extract_or_generate_correlation_id, generate_xray_compatible_trace_id,
};
pub use emf::{EmfMetric, emit_emf, emit_emf_to};
pub use idempotency::{
    IdempotencyError, IdempotencyRecord, IdempotencyStore, InMemoryIdempotencyStore,
};
pub use logger::{init, init_with_writer, json_subscriber};

#[doc(hidden)]
pub mod __private {
    pub use serde_json;
}
