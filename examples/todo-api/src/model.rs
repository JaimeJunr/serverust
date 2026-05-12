use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

/// Tarefa retornada pela API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub done: bool,
    /// Epoch em segundos.
    pub created_at: u64,
}

/// Payload para criar uma nova tarefa.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateTaskDto {
    /// Título da tarefa, 1-200 caracteres.
    #[validate(length(min = 1, max = 200))]
    #[schema(min_length = 1, max_length = 200)]
    pub title: String,
}

/// Payload para atualizar uma tarefa existente. Todos os campos são opcionais.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateTaskDto {
    #[validate(length(min = 1, max = 200))]
    #[schema(min_length = 1, max_length = 200)]
    pub title: Option<String>,
    pub done: Option<bool>,
}
