use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;
use serverust_core::extract::Json;
use serverust_macros::{delete, get, post, put};

use super::model::CreateFundDto;
use super::service::FundsService;

#[get("/funds")]
pub async fn list_funds(State(svc): State<Arc<FundsService>>) -> impl IntoResponse {
    axum::Json(svc.list())
}

#[post("/funds")]
pub async fn create_fund(
    State(svc): State<Arc<FundsService>>,
    Json(dto): Json<CreateFundDto>,
) -> impl IntoResponse {
    let fund = svc.create(dto);
    (StatusCode::CREATED, axum::Json(fund))
}

#[get("/funds/{id}")]
pub async fn get_fund(
    Path(id): Path<u64>,
    State(svc): State<Arc<FundsService>>,
) -> impl IntoResponse {
    match svc.get(id) {
        Some(fund) => axum::Json(fund).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "fund_not_found"})),
        )
            .into_response(),
    }
}

#[put("/funds/{id}")]
pub async fn update_fund(
    Path(id): Path<u64>,
    State(svc): State<Arc<FundsService>>,
    Json(dto): Json<CreateFundDto>,
) -> impl IntoResponse {
    match svc.update(id, dto) {
        Some(fund) => axum::Json(fund).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "fund_not_found"})),
        )
            .into_response(),
    }
}

#[delete("/funds/{id}")]
pub async fn delete_fund(
    Path(id): Path<u64>,
    State(svc): State<Arc<FundsService>>,
) -> impl IntoResponse {
    if svc.delete(id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "fund_not_found"})),
        )
            .into_response()
    }
}
