//! API de tarefas (Todo) — exemplo didático para o tutorial Lambda do serverust.
//!
//! Expõe um CRUD completo em memória de [`Task`] com:
//! - Validação de entrada via `#[derive(Validate)]`
//! - Erros padronizados com `#[derive(ApiError)]`
//! - Dependency Injection do `TaskService`
//! - OpenAPI / Swagger UI / ReDoc automáticos
//! - Mesma binária roda local (HTTP) e em AWS Lambda
//!
//! O `main` binário registra as rotas; este `lib` permite escrever testes de
//! integração contra o `axum::Router` sem subir porta.

pub mod errors;
pub mod handlers;
pub mod model;
pub mod service;

use std::sync::Arc;

use serverust_core::App;

use crate::handlers::{create_task, delete_task, get_task, list_tasks, update_task};
use crate::model::{CreateTaskDto, Task, UpdateTaskDto};
use crate::service::TaskService;

/// Constrói a `App` configurada deste exemplo. Reutilizado por `main.rs` e por testes.
pub fn build_app() -> App {
    App::new()
        .openapi_info("Todo API", "0.1.0")
        .register_schema::<Task>()
        .register_schema::<CreateTaskDto>()
        .register_schema::<UpdateTaskDto>()
        .provide::<TaskService>(Arc::new(TaskService::new()))
        .route(list_tasks)
        .route(create_task)
        .route(get_task)
        .route(update_task)
        .route(delete_task)
}
