//! Telemetria nativa do RustAPI: logger JSON estruturado, propagação de
//! correlation id compatível com AWS X-Ray, métricas em formato EMF e trait
//! de idempotência.
//!
//! O ponto de entrada típico é [`init`], chamado uma vez no boot. As demais
//! peças (middleware de correlation id, helpers EMF e [`IdempotencyStore`])
//! são opcionais e podem ser combinadas conforme a necessidade.
//!
//! Integrações pesadas (OpenTelemetry/X-Ray e DynamoDB) ficam atrás das
//! features `otel` e `dynamodb` para manter o binário default enxuto.

pub mod correlation;
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
