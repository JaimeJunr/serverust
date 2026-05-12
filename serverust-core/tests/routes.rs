use axum::body::Body;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serverust_core::App;
use serverust_core::extract::{Json, Path, Query};
use serverust_macros::{get, post};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use validator::Validate;

#[get("/hello")]
async fn hello() -> &'static str {
    "hi"
}

#[get("/users/{id}")]
async fn get_user(Path(id): Path<u32>) -> String {
    format!("user-{id}")
}

#[derive(Deserialize, Serialize, Validate)]
struct CreateUser {
    name: String,
}

#[post("/users")]
async fn create_user(Json(body): Json<CreateUser>) -> Json<CreateUser> {
    Json(body)
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

#[get("/search")]
async fn search(Query(q): Query<SearchQuery>) -> String {
    q.q
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn get_route_returns_static_string() {
    let router = App::new().route(hello).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "hi");
}

#[tokio::test]
async fn path_param_is_extracted() {
    let router = App::new().route(get_user).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/users/42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "user-42");
}

#[tokio::test]
async fn post_with_json_body_round_trips() {
    let router = App::new().route(create_user).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/users")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"alice"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"name\":\"alice\""), "got: {body}");
}

#[tokio::test]
async fn query_extractor_works() {
    let router = App::new().route(search).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/search?q=rust")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "rust");
}

#[tokio::test]
async fn multiple_routes_compose() {
    let router = App::new()
        .route(hello)
        .route(get_user)
        .route(create_user)
        .into_router();

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/users/7")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn run_http_binds_local_socket() {
    let app = App::new().route(hello);

    let result = tokio::time::timeout(
        std::time::Duration::from_millis(150),
        app.run_http("127.0.0.1:0"),
    )
    .await;

    assert!(
        result.is_err(),
        "run_http should run until cancelled; got: {result:?}"
    );
}
