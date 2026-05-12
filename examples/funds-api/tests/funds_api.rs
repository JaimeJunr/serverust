use std::sync::Arc;

use funds_api::modules::funds::{
    handlers::{create_fund, delete_fund, get_fund, list_funds, update_fund},
    service::FundsService,
};
use http_body_util::BodyExt;
use serverust_core::App;
use tower::ServiceExt;

fn build_app() -> axum::Router {
    App::new()
        .openapi_info("Funds API", "1.0.0")
        .provide::<FundsService>(Arc::new(FundsService::new()))
        .route(list_funds)
        .route(create_fund)
        .route(get_fund)
        .route(update_fund)
        .route(delete_fund)
        .into_router()
}

#[tokio::test]
async fn test_list_funds_empty() {
    let app = build_app();
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/funds")
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), 200);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let funds: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(funds, serde_json::json!([]));
}

#[tokio::test]
async fn test_create_fund_returns_201() {
    let app = build_app();
    let payload = serde_json::json!({
        "name": "Fundo XP Ações",
        "cnpj": "12.345.678/0001-90",
        "nav": 150.50
    });
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/funds")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(payload.to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), 201);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let fund: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(fund["name"], "Fundo XP Ações");
    assert!(fund["id"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_create_fund_validation_error_422() {
    let app = build_app();
    // name vazio é inválido (min_length = 1)
    let payload = serde_json::json!({
        "name": "",
        "cnpj": "12.345.678/0001-90",
        "nav": 100.0
    });
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/funds")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(payload.to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), 422);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["error"], "validation_error");
    assert!(err["fields"]["name"].is_array());
}

#[tokio::test]
async fn test_get_fund_not_found_404() {
    let app = build_app();
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/funds/999")
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), 404);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["error"], "fund_not_found");
}

#[tokio::test]
async fn test_delete_fund_not_found_404() {
    let app = build_app();
    let req = axum::http::Request::builder()
        .method("DELETE")
        .uri("/funds/999")
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), 404);
}
