//! Correlation id: extração pura + propagação no middleware axum.

use axum::Router;
use axum::body::Body;
use axum::routing::get;
use http::{HeaderMap, HeaderValue, Request};
use serverust_telemetry::{
    CORRELATION_ID_HEADER, X_AMZN_TRACE_ID, correlation_id_layer,
    extract_or_generate_correlation_id, generate_xray_compatible_trace_id,
};
use tower::ServiceExt;

#[test]
fn extracts_xray_root_field_from_amzn_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        X_AMZN_TRACE_ID,
        HeaderValue::from_static("Root=1-5e988a0d-1234567890abcdef12345678;Parent=foo;Sampled=1"),
    );
    let id = extract_or_generate_correlation_id(&headers);
    assert_eq!(id, "1-5e988a0d-1234567890abcdef12345678");
}

#[test]
fn extracts_correlation_id_header_when_xray_absent() {
    let mut headers = HeaderMap::new();
    headers.insert(CORRELATION_ID_HEADER, HeaderValue::from_static("custom-id"));
    let id = extract_or_generate_correlation_id(&headers);
    assert_eq!(id, "custom-id");
}

#[test]
fn generates_xray_format_when_no_headers_present() {
    let headers = HeaderMap::new();
    let id = extract_or_generate_correlation_id(&headers);
    assert_xray_format(&id);
}

#[test]
fn generated_trace_id_matches_xray_format() {
    for _ in 0..5 {
        let id = generate_xray_compatible_trace_id();
        assert_xray_format(&id);
    }
}

fn assert_xray_format(id: &str) {
    // Formato esperado: `1-<8 hex>-<24 hex>`.
    let parts: Vec<&str> = id.split('-').collect();
    assert_eq!(parts.len(), 3, "id inválido: {id}");
    assert_eq!(parts[0], "1");
    assert_eq!(parts[1].len(), 8);
    assert_eq!(parts[2].len(), 24);
    assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));
    assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn middleware_propagates_correlation_id_to_response() {
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(correlation_id_layer!());

    let request = Request::builder()
        .uri("/")
        .header(
            X_AMZN_TRACE_ID,
            "Root=1-aabbccdd-001122334455667788990011;Sampled=0",
        )
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let header = response
        .headers()
        .get(CORRELATION_ID_HEADER)
        .expect("response deve conter X-Correlation-Id");
    assert_eq!(header, "1-aabbccdd-001122334455667788990011");
}

#[tokio::test]
async fn middleware_generates_id_when_no_header_provided() {
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(correlation_id_layer!());

    let request = Request::builder().uri("/").body(Body::empty()).unwrap();
    let response = app.oneshot(request).await.unwrap();
    let header = response
        .headers()
        .get(CORRELATION_ID_HEADER)
        .expect("response deve conter X-Correlation-Id");
    let header_str = header.to_str().unwrap();
    let parts: Vec<&str> = header_str.split('-').collect();
    assert_eq!(parts.len(), 3, "id inválido: {header_str}");
    assert_eq!(parts[0], "1");
}
