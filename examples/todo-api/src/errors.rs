use serverust_macros::ApiError;

/// Erros de domínio da API de tarefas. Cada variante é mapeada para um status HTTP
/// automaticamente pelo derive `ApiError`.
#[derive(Debug, ApiError)]
pub enum TaskError {
    #[status(404)]
    #[message("Task não encontrada")]
    NotFound,
}
