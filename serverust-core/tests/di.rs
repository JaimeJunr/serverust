//! Testes de Dependency Injection: provide, override e ciclo de vida Singleton.

use axum::body::Body;
use axum::extract::State;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serverust_core::App;
use serverust_macros::{get, injectable};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

// Trait usada como contrato injetável. `Send + Sync` é necessário para
// `Arc<dyn Counter>` poder atravessar o axum.
trait Counter: Send + Sync {
    fn increment(&self) -> usize;
}

// Implementação "real" — registra estado interno para comprovar Singleton.
#[injectable]
#[derive(Default)]
struct AtomicCounter {
    value: AtomicUsize,
}

impl Counter for AtomicCounter {
    fn increment(&self) -> usize {
        self.value.fetch_add(1, Ordering::SeqCst) + 1
    }
}

// Mock que sempre devolve um valor fixo — útil para teste de override.
#[injectable]
struct StubCounter;

impl Counter for StubCounter {
    fn increment(&self) -> usize {
        999
    }
}

#[get("/count")]
async fn count_handler(State(counter): State<Arc<dyn Counter>>) -> String {
    format!("{}", counter.increment())
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn call_count(router: axum::Router) -> String {
    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/count")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    body_string(resp).await
}

#[tokio::test]
async fn provide_injects_arc_dyn_trait_into_handler() {
    let counter: Arc<dyn Counter> = Arc::new(AtomicCounter::default());
    let router = App::new()
        .provide::<dyn Counter>(counter)
        .route(count_handler)
        .into_router();

    assert_eq!(call_count(router).await, "1");
}

#[tokio::test]
async fn override_replaces_previously_provided_service() {
    let real: Arc<dyn Counter> = Arc::new(AtomicCounter::default());
    let mock: Arc<dyn Counter> = Arc::new(StubCounter);
    let router = App::new()
        .provide::<dyn Counter>(real)
        .r#override::<dyn Counter>(mock)
        .route(count_handler)
        .into_router();

    assert_eq!(call_count(router).await, "999");
}

#[tokio::test]
async fn singleton_is_shared_across_invocations() {
    let counter: Arc<dyn Counter> = Arc::new(AtomicCounter::default());
    let router = App::new()
        .provide::<dyn Counter>(counter)
        .route(count_handler)
        .into_router();

    assert_eq!(call_count(router.clone()).await, "1");
    assert_eq!(call_count(router.clone()).await, "2");
    assert_eq!(call_count(router).await, "3");
}

#[tokio::test]
async fn injectable_marker_trait_is_implemented() {
    // Tipos anotados com `#[injectable]` recebem o marker trait `Injectable`
    // — confirma que a macro emitiu a impl sem alterar o tipo original.
    fn assert_injectable<T: serverust_core::Injectable>() {}
    assert_injectable::<AtomicCounter>();
    assert_injectable::<StubCounter>();
}
