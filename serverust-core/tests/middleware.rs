//! Testes da pipeline declarativa: Guards, Pipes e Interceptors.
//!
//! Cobrem cada primitiva isoladamente e a composição completa, validando que
//! a ordem efetiva de execução é Guards → Pipes → Handler → Interceptors.

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use http::{Method, Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serverust_core::{App, Guard, Interceptor, ParseUuidPipe, PipePath};
use serverust_macros::{get, guard};
use tower::ServiceExt;
use uuid::Uuid;

// Guard que aprova apenas quando o header `x-token: secret` está presente.
struct AuthGuard;

impl Guard for AuthGuard {
    async fn check(parts: &Parts) -> Result<(), Response> {
        if parts.headers.get("x-token").is_some_and(|v| v == "secret") {
            Ok(())
        } else {
            Err((StatusCode::UNAUTHORIZED, "unauthorized").into_response())
        }
    }
}

#[guard(AuthGuard)]
#[get("/admin")]
async fn admin_handler() -> &'static str {
    "secret-area"
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn req_get(path: &str) -> HttpRequest<Body> {
    HttpRequest::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn guard_blocks_handler_when_check_fails() {
    let router = App::new().route(admin_handler).into_router();

    let resp = router.oneshot(req_get("/admin")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_string(resp).await;
    assert_eq!(
        body, "unauthorized",
        "body precisa vir do guard, não do handler"
    );
    assert!(
        !body.contains("secret-area"),
        "handler não pode ter sido executado"
    );
}

#[tokio::test]
async fn guard_allows_handler_when_check_passes() {
    let router = App::new().route(admin_handler).into_router();

    let req = HttpRequest::builder()
        .method(Method::GET)
        .uri("/admin")
        .header("x-token", "secret")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "secret-area");
}

#[get("/user/{id}")]
async fn get_user(PipePath(id): PipePath<ParseUuidPipe>) -> String {
    format!("uuid={id}")
}

#[tokio::test]
async fn pipe_transforms_valid_input_before_handler() {
    let router = App::new().route(get_user).into_router();
    let uuid = Uuid::nil();

    let resp = router
        .oneshot(req_get(&format!("/user/{uuid}")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, format!("uuid={uuid}"));
}

#[tokio::test]
async fn pipe_rejects_invalid_input_with_400() {
    let router = App::new().route(get_user).into_router();

    let resp = router.oneshot(req_get("/user/not-a-uuid")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// Interceptor de "timing": adiciona um header marcador na resposta. O valor
// real não importa para o teste; o que comprova o wrap é o header existir.
#[derive(Clone)]
struct TimingInterceptor;

impl Interceptor for TimingInterceptor {
    async fn intercept(&self, req: Request, next: Next) -> Response {
        let mut resp = next.run(req).await;
        resp.headers_mut()
            .insert("x-intercepted", "true".parse().unwrap());
        resp
    }
}

#[get("/ping")]
async fn ping() -> &'static str {
    "pong"
}

#[tokio::test]
async fn interceptor_wraps_response_and_adds_header() {
    let router = App::new()
        .route(ping)
        .interceptor(TimingInterceptor)
        .into_router();

    let resp = router.oneshot(req_get("/ping")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("x-intercepted")
            .and_then(|v| v.to_str().ok()),
        Some("true"),
        "interceptor deveria injetar o header x-intercepted"
    );
}

// --- Composição: Guard + Pipe + Interceptor todos juntos --------------------

#[guard(AuthGuard)]
#[get("/secret/{id}")]
async fn protected_user(PipePath(id): PipePath<ParseUuidPipe>) -> String {
    format!("ok:{id}")
}

#[tokio::test]
async fn full_pipeline_executes_in_order_guard_pipe_handler_interceptor() {
    let router = App::new()
        .route(protected_user)
        .interceptor(TimingInterceptor)
        .into_router();

    let uuid = Uuid::nil();

    // 1. Sem token → guard bloqueia, handler não chamado, MAS interceptor
    //    ainda envolve a resposta (wraps tudo, incluindo rejection do guard).
    let resp = router
        .clone()
        .oneshot(req_get(&format!("/secret/{uuid}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        resp.headers()
            .get("x-intercepted")
            .and_then(|v| v.to_str().ok()),
        Some("true"),
        "interceptor envelopa toda a execução"
    );
    assert_eq!(
        body_string(resp).await,
        "unauthorized",
        "rejection veio do guard, não do handler"
    );

    // 2. Com token e UUID válido → tudo passa.
    let req = HttpRequest::builder()
        .method(Method::GET)
        .uri(format!("/secret/{uuid}"))
        .header("x-token", "secret")
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, format!("ok:{uuid}"));

    // 3. Com token MAS UUID inválido → guard passa, pipe rejeita (400).
    let req = HttpRequest::builder()
        .method(Method::GET)
        .uri("/secret/not-a-uuid")
        .header("x-token", "secret")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_string(resp).await;
    assert!(
        body.contains("invalid_uuid"),
        "body deve indicar erro do pipe, não do handler: {body}"
    );
}
