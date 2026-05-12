//! Propagação de correlation id compatível com AWS X-Ray.
//!
//! A função [`extract_or_generate_correlation_id`] é o núcleo puro: dado o
//! mapa de headers, devolve o valor de `X-Amzn-Trace-Id` (formato X-Ray) ou
//! gera um trace id novo no mesmo formato (`1-<8 hex>-<24 hex>`).
//!
//! [`correlation_id_middleware`] é o middleware axum pronto para aplicar
//! via `axum::middleware::from_fn(correlation_id_middleware)`: aplica o id
//! à request entrante, propaga via header de resposta e abre um span
//! tracing com o campo `correlation_id` — garantindo que cada log emitido
//! durante o request carregue o id.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use http::HeaderMap;
use http::header::HeaderName;
use tracing::Instrument;

/// Header padrão do X-Ray. Quando presente vira o correlation id.
pub const X_AMZN_TRACE_ID: HeaderName = HeaderName::from_static("x-amzn-trace-id");
/// Header espelhado no request/response para clientes não-AWS consumirem.
pub const CORRELATION_ID_HEADER: HeaderName = HeaderName::from_static("x-correlation-id");

/// Extrai `X-Amzn-Trace-Id` ou gera um trace id no formato X-Ray.
///
/// O valor do header X-Ray costuma ser `Root=1-abc-def;Parent=...;Sampled=1`;
/// devolvemos o campo `Root=` quando reconhecemos, caso contrário o header
/// inteiro (mantém compatibilidade com clientes que mandam só o id).
pub fn extract_or_generate_correlation_id(headers: &HeaderMap) -> String {
    if let Some(value) = headers.get(&X_AMZN_TRACE_ID)
        && let Ok(text) = value.to_str()
    {
        if let Some(root) = parse_xray_root(text) {
            return root;
        }
        return text.to_string();
    }
    if let Some(value) = headers.get(&CORRELATION_ID_HEADER)
        && let Ok(text) = value.to_str()
    {
        return text.to_string();
    }
    generate_xray_compatible_trace_id()
}

fn parse_xray_root(header: &str) -> Option<String> {
    for segment in header.split(';') {
        let segment = segment.trim();
        if let Some(value) = segment.strip_prefix("Root=") {
            return Some(value.to_string());
        }
    }
    None
}

/// Gera um trace id novo no formato AWS X-Ray:
/// `1-<8 hex epoch seconds>-<24 hex random>`. Compatível com qualquer
/// consumidor que valide o formato X-Ray (CloudWatch, AWS console).
pub fn generate_xray_compatible_trace_id() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    // 24 hex chars = 96 bits aleatórios; reusamos uuid_v4 (122 bits) e
    // pegamos os 24 primeiros hex chars — entropia mais que suficiente para
    // correlation id.
    let random = uuid::Uuid::new_v4().simple().to_string();
    let random_24 = &random[..24];
    format!("1-{epoch:08x}-{random_24}")
}

/// Middleware axum que garante presença do correlation id no request,
/// propaga via `X-Correlation-Id` no response e abre um span tracing.
///
/// Aplique via `axum::middleware::from_fn(correlation_id_middleware)` ou
/// chame o helper [`correlation_id_layer!`](crate::correlation_id_layer).
pub async fn correlation_id_middleware(mut req: Request, next: Next) -> Response {
    let id = extract_or_generate_correlation_id(req.headers());
    if let Ok(value) = id.parse() {
        req.headers_mut().insert(CORRELATION_ID_HEADER, value);
    }
    let span = tracing::info_span!("request", correlation_id = %id);
    let mut response = next.run(req).instrument(span).await;
    if let Ok(value) = id.parse() {
        response.headers_mut().insert(CORRELATION_ID_HEADER, value);
    }
    response
}

/// Macro de conveniência que constrói a layer correlation id pronta para
/// passar em `Router::layer(...)`. Mantemos como macro (e não função) porque
/// o tipo retornado por `axum::middleware::from_fn` depende da função
/// concreta, que não é nomeável de forma estável entre versões.
#[macro_export]
macro_rules! correlation_id_layer {
    () => {
        ::axum::middleware::from_fn($crate::correlation::correlation_id_middleware)
    };
}
