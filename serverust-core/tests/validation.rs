use axum::body::Body;
use axum::response::IntoResponse;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serverust_core::App;
use serverust_core::extract::Json;
use serverust_macros::{ApiError, post};
use tower::ServiceExt;
use validator::Validate;

#[derive(Deserialize, Serialize, Validate)]
struct SignupBody {
    #[validate(length(min = 3, message = "must be at least 3 chars"))]
    name: String,
    #[validate(email(message = "must be a valid email"))]
    email: String,
}

#[post("/signup")]
async fn signup(Json(body): Json<SignupBody>) -> Json<SignupBody> {
    Json(body)
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn happy_path_validates_and_returns_payload() {
    let router = App::new().route(signup).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/signup")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"alice","email":"alice@example.com"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains("alice@example.com"));
}

#[tokio::test]
async fn validation_failure_returns_422_with_structured_payload() {
    let router = App::new().route(signup).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/signup")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"a","email":"not-an-email"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let json = body_json(resp).await;
    assert_eq!(json["error"], "validation_error");

    let fields = &json["fields"];
    assert!(fields.is_object(), "fields must be an object: {json}");

    let name_msgs = fields["name"].as_array().expect("name must be array");
    assert!(name_msgs.iter().any(|m| m == "must be at least 3 chars"));

    let email_msgs = fields["email"].as_array().expect("email must be array");
    assert!(email_msgs.iter().any(|m| m == "must be a valid email"));
}

#[derive(Debug, ApiError)]
enum BillingError {
    #[status(404)]
    #[message("invoice not found")]
    NotFound,
    #[status(409)]
    #[message("invoice already paid")]
    AlreadyPaid,
}

#[post("/charge")]
async fn charge(Json(body): Json<SignupBody>) -> Result<Json<SignupBody>, BillingError> {
    if body.name == "missing" {
        return Err(BillingError::NotFound);
    }
    if body.name == "paid" {
        return Err(BillingError::AlreadyPaid);
    }
    Ok(Json(body))
}

#[tokio::test]
async fn api_error_variant_maps_to_status_and_message() {
    let router = App::new().route(charge).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/charge")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"missing","email":"a@b.io"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let json = body_json(resp).await;
    assert_eq!(json["error"], "invoice not found");
}

#[tokio::test]
async fn api_error_second_variant_maps_to_409() {
    let router = App::new().route(charge).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/charge")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"paid","email":"a@b.io"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let json = body_json(resp).await;
    assert_eq!(json["error"], "invoice already paid");
}

#[tokio::test]
async fn api_error_implements_into_response_directly() {
    // Garante que a derive expõe IntoResponse para uso fora de handlers Result<>.
    let resp = BillingError::NotFound.into_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
