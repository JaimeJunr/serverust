use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serverust_core::extract::Json;
use serverust_macros::{delete, get, post, put};

use crate::errors::TaskError;
use crate::model::{CreateTaskDto, Task, UpdateTaskDto};
use crate::service::TaskService;

/// Lista todas as tarefas.
#[get("/tasks")]
pub async fn list_tasks(State(svc): State<Arc<TaskService>>) -> Json<Vec<Task>> {
    Json(svc.list())
}

/// Cria uma nova tarefa. Validação automática (HTTP 422 se inválido).
#[post("/tasks")]
pub async fn create_task(
    State(svc): State<Arc<TaskService>>,
    Json(dto): Json<CreateTaskDto>,
) -> impl IntoResponse {
    let task = svc.create(dto);
    (StatusCode::CREATED, Json(task))
}

/// Busca uma tarefa por id. HTTP 404 padronizado se não existir.
#[get("/tasks/{id}")]
pub async fn get_task(
    Path(id): Path<u64>,
    State(svc): State<Arc<TaskService>>,
) -> Result<Json<Task>, TaskError> {
    svc.get(id).map(Json).ok_or(TaskError::NotFound)
}

/// Atualiza campos da tarefa. HTTP 404 se não existir.
#[put("/tasks/{id}")]
pub async fn update_task(
    Path(id): Path<u64>,
    State(svc): State<Arc<TaskService>>,
    Json(dto): Json<UpdateTaskDto>,
) -> Result<Json<Task>, TaskError> {
    svc.update(id, dto).map(Json).ok_or(TaskError::NotFound)
}

/// Remove uma tarefa. HTTP 204 ou 404.
#[delete("/tasks/{id}")]
pub async fn delete_task(
    Path(id): Path<u64>,
    State(svc): State<Arc<TaskService>>,
) -> Result<StatusCode, TaskError> {
    if svc.delete(id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(TaskError::NotFound)
    }
}
