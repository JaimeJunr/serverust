//! Integração opcional com OpenTelemetry usando o gerador de trace id do
//! AWS X-Ray. Ativada pela feature `otel`.
//!
//! Como o `tracing-subscriber` é instalado globalmente via [`crate::init`],
//! aqui apenas devolvemos o `Tracer` configurado para X-Ray e o propagador;
//! o usuário compõe a camada `tracing-opentelemetry` no seu próprio
//! `Subscriber` quando precisar substituir o subscriber default.

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_aws::trace::{XrayIdGenerator, XrayPropagator};
use opentelemetry_sdk::trace::{Config, Tracer, TracerProvider};

/// Configura propagador X-Ray globalmente e devolve um `Tracer` pronto
/// para compor em `tracing-opentelemetry`. Caller controla shutdown via
/// o `TracerProvider`.
pub fn init_xray(service_name: &'static str) -> (TracerProvider, Tracer) {
    global::set_text_map_propagator(XrayPropagator::default());
    let provider = TracerProvider::builder()
        .with_config(Config::default().with_id_generator(XrayIdGenerator::default()))
        .build();
    let tracer = provider.tracer(service_name);
    (provider, tracer)
}
