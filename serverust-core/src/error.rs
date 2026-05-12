use axum::Json as AxumJson;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde_json::{Map, Value, json};
use validator::ValidationErrors;

/// Trait implementada pelo `#[derive(ApiError)]`.
///
/// Permite mapear variantes de erro para status HTTP + mensagem padronizada.
/// A derive emite, junto com este impl, um `IntoResponse` para que `?` em
/// handlers `Result<T, E: ApiError>` converta a falha em resposta.
pub trait ApiError {
    fn status(&self) -> u16;
    fn message(&self) -> String;
}

/// Constrói a resposta HTTP 422 padronizada a partir de erros do validator.
///
/// Formato: `{ "error": "validation_error", "fields": { campo: [mensagens] } }`.
pub fn validation_error_response(errors: &ValidationErrors) -> Response {
    let mut fields = Map::new();

    for (field, kind) in errors.field_errors() {
        let messages: Vec<Value> = kind
            .iter()
            .map(|e| {
                let msg = e
                    .message
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| e.code.to_string());
                Value::String(msg)
            })
            .collect();
        fields.insert((*field).to_string(), Value::Array(messages));
    }

    let body = json!({
        "error": "validation_error",
        "fields": fields,
    });

    (StatusCode::UNPROCESSABLE_ENTITY, AxumJson(body)).into_response()
}
