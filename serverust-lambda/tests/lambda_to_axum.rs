//! Integração: parsea um evento de API Gateway / Function URL via
//! `lambda_http::request::from_str` e roda a requisição pelo `Router` gerado
//! pela App, validando que o roteamento e os extractors do framework
//! funcionam idênticos ao modo HTTP local.

use axum::body::Body;
use axum::extract::Request as AxumRequest;
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use serverust_core::App;
use serverust_core::extract::{Json, Path, Query};
use serverust_macros::{get, post};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use utoipa::ToSchema;
use validator::Validate;

#[derive(Deserialize, Serialize, Validate, ToSchema)]
struct Echo {
    #[validate(length(min = 1))]
    name: String,
}

#[derive(Deserialize, Serialize)]
struct HelloParams {
    name: Option<String>,
}

#[get("/hello")]
async fn hello(Query(params): Query<HelloParams>) -> String {
    format!("hello, {}", params.name.as_deref().unwrap_or("world"))
}

#[get("/items/{id}")]
async fn show_item(Path(id): Path<u32>) -> String {
    format!("item {id}")
}

#[post("/echo")]
async fn echo(Json(payload): Json<Echo>) -> impl IntoResponse {
    format!("echo {}", payload.name)
}

fn build_router() -> axum::Router {
    App::new()
        .route(hello)
        .route(show_item)
        .route(echo)
        .into_router()
}

/// Converte um `lambda_http::Request` em `axum::Request` preservando method,
/// uri, headers e body. Esta é a mesma conversão que `lambda_http::run`
/// executa internamente antes de invocar o service axum.
fn into_axum_request(req: lambda_http::Request) -> AxumRequest {
    let (parts, body) = req.into_parts();
    let bytes: Vec<u8> = match body {
        lambda_http::Body::Empty => Vec::new(),
        lambda_http::Body::Text(s) => s.into_bytes(),
        lambda_http::Body::Binary(b) => b,
        // lambda_http::Body é `#[non_exhaustive]`; futuras variantes caem em
        // body vazio sem perder a requisição.
        _ => Vec::new(),
    };
    AxumRequest::from_parts(parts, Body::from(bytes))
}

async fn body_to_string(response: axum::response::Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn api_gateway_v1_request_routes_through_axum() {
    let fixture = include_str!("fixtures/apigw_v1_get.json");
    let req: lambda_http::Request =
        lambda_http::request::from_str(fixture).expect("fixture v1 deve parsear");

    let router = build_router();
    let response = router
        .oneshot(into_axum_request(req))
        .await
        .expect("router responde");

    assert_eq!(response.status(), 200);
    assert_eq!(body_to_string(response).await, "hello, world");
}

#[tokio::test]
async fn api_gateway_v2_post_with_json_body_routes_through_axum() {
    let fixture = include_str!("fixtures/apigw_v2_post.json");
    let req: lambda_http::Request =
        lambda_http::request::from_str(fixture).expect("fixture v2 deve parsear");

    let router = build_router();
    let response = router
        .oneshot(into_axum_request(req))
        .await
        .expect("router responde");

    assert_eq!(response.status(), 200);
    assert_eq!(body_to_string(response).await, "echo world");
}

#[tokio::test]
async fn lambda_function_url_request_routes_through_axum() {
    let fixture = include_str!("fixtures/lambda_function_url_get.json");
    let req: lambda_http::Request =
        lambda_http::request::from_str(fixture).expect("fixture function URL deve parsear");

    let router = build_router();
    let response = router
        .oneshot(into_axum_request(req))
        .await
        .expect("router responde");

    assert_eq!(response.status(), 200);
    assert_eq!(body_to_string(response).await, "hello, lambda");
}
