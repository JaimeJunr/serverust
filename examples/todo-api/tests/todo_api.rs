use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use todo_api::build_app;

async fn body_json(body: Body) -> Value {
    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn lista_vazia_inicialmente() {
    let app = build_app().into_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp.into_body()).await, json!([]));
}

#[tokio::test]
async fn cria_busca_atualiza_remove() {
    let app = build_app().into_router();

    // CREATE
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tasks")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"escrever doc"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp.into_body()).await;
    assert_eq!(created["title"], "escrever doc");
    assert_eq!(created["done"], false);
    let id = created["id"].as_u64().unwrap();

    // GET
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/tasks/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // UPDATE: marca como concluída
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/tasks/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"done":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = body_json(resp.into_body()).await;
    assert_eq!(updated["done"], true);

    // DELETE
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/tasks/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn payload_invalido_devolve_422() {
    let app = build_app().into_router();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tasks")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":""}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["error"], "validation_error");
    assert!(body["fields"]["title"].is_array());
}

#[tokio::test]
async fn id_inexistente_devolve_404() {
    let app = build_app().into_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/tasks/999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn openapi_inclui_rotas_de_tasks() {
    let app = build_app().into_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let spec = body_json(resp.into_body()).await;
    assert!(spec["paths"]["/tasks"].is_object());
    assert!(spec["paths"]["/tasks/{id}"].is_object());
}
