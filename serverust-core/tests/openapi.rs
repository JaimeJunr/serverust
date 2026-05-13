use axum::body::Body;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serverust_core::App;
use serverust_core::extract::{Json, Path};
use serverust_macros::{get, post};
use tower::ServiceExt;
use utoipa::ToSchema;
use validator::Validate;

#[get("/hello")]
async fn hello() -> &'static str {
    "hi"
}

#[get("/users/{id}")]
async fn get_user(Path(id): Path<u32>) -> String {
    format!("user-{id}")
}

#[derive(Deserialize, Serialize, Validate, ToSchema)]
struct CreateUser {
    #[validate(length(min = 3, max = 50))]
    #[schema(min_length = 3, max_length = 50)]
    name: String,
}

#[post("/users")]
async fn create_user(Json(body): Json<CreateUser>) -> Json<CreateUser> {
    Json(body)
}

#[post(
    "/auth/login",
    response = CreateUser,
    tag = "auth",
    operation_id = "loginUser",
    request_example = r#"{"name":"jaime"}"#,
    response_example = r#"{"name":"jaime"}"#
)]
async fn login(Json(body): Json<CreateUser>) -> Json<CreateUser> {
    Json(body)
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn openapi_json_lists_registered_endpoints() {
    let router = App::new()
        .route(hello)
        .route(get_user)
        .route(create_user)
        .into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;

    assert_eq!(json["openapi"], Value::String("3.1.0".into()));
    let paths = json["paths"].as_object().expect("paths object");
    assert!(paths.contains_key("/hello"), "missing /hello: {json}");
    assert!(
        paths.contains_key("/users/{id}"),
        "missing /users/{{id}}: {json}"
    );
    assert!(paths.contains_key("/users"), "missing /users: {json}");

    assert!(paths["/hello"].get("get").is_some());
    assert!(paths["/users/{id}"].get("get").is_some());
    assert!(paths["/users"].get("post").is_some());
}

#[tokio::test]
async fn openapi_info_customizes_title_and_version() {
    let router = App::new()
        .openapi_info("Funds API", "2.4.0")
        .route(hello)
        .into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = body_json(resp).await;
    assert_eq!(json["info"]["title"], Value::String("Funds API".into()));
    assert_eq!(json["info"]["version"], Value::String("2.4.0".into()));
}

#[tokio::test]
async fn registered_schemas_include_validate_constraints() {
    let router = App::new()
        .register_schema::<CreateUser>()
        .route(create_user)
        .into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = body_json(resp).await;
    let schema = &json["components"]["schemas"]["CreateUser"];
    assert!(
        schema.is_object(),
        "CreateUser schema missing: {}",
        serde_json::to_string_pretty(&json).unwrap()
    );

    let required = schema["required"].as_array().expect("required array");
    assert!(
        required.iter().any(|r| r == "name"),
        "required should contain name; got: {schema}"
    );

    let name_prop = &schema["properties"]["name"];
    assert_eq!(name_prop["minLength"], Value::Number(3.into()));
    assert_eq!(name_prop["maxLength"], Value::Number(50.into()));
}

#[tokio::test]
async fn swagger_ui_is_served_at_docs() {
    let router = App::new().route(hello).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/docs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    assert!(ct.starts_with("text/html"), "content-type was {ct}");

    let body = body_string(resp).await;
    assert!(
        body.to_lowercase().contains("scalar") || body.to_lowercase().contains("api-reference"),
        "body should mention scalar/api-reference"
    );
    assert!(
        body.contains("/openapi.json"),
        "swagger should point to /openapi.json"
    );
}

#[tokio::test]
async fn redoc_is_served_at_redoc() {
    let router = App::new().route(hello).into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/redoc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.to_lowercase().contains("redoc"));
    assert!(body.contains("/openapi.json"));
}

#[tokio::test]
async fn docs_path_can_be_customized() {
    let router = App::new().docs("/swagger").route(hello).into_router();

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/swagger")
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
                .uri("/docs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn operation_supports_tag_operation_id_examples_and_default_errors() {
    let router = App::new().route(login).into_router();
    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = body_json(resp).await;
    let op = &json["paths"]["/auth/login"]["post"];
    assert_eq!(op["operationId"], Value::String("loginUser".into()));
    assert_eq!(op["tags"], Value::Array(vec![Value::String("auth".into())]));
    assert!(
        op["description"]
            .as_str()
            .unwrap_or_default()
            .contains("Request example")
    );
    assert!(op["responses"].get("401").is_some());
    assert!(op["responses"].get("403").is_some());
    assert!(op["responses"].get("422").is_some());
    assert!(op["responses"].get("500").is_some());
}
