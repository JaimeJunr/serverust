use axum::extract::{FromRequest, Request};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error::validation_error_response;

/// Extractor JSON com validação automática.
///
/// Deserializa o body como `T` e, se a validação via `validator::Validate`
/// falhar, devolve HTTP 422 com payload padronizado **antes** do handler ser
/// invocado:
///
/// ```json
/// { "error": "validation_error", "fields": { "title": ["length"] } }
/// ```
///
/// `T` precisa derivar `Deserialize` **e** `Validate`. Se você não tem regras
/// de validação, o derive `Validate` continua sendo no-op:
///
/// ```ignore
/// use serde::Deserialize;
/// use validator::Validate;
/// use serverust_core::extract::Json;
/// use serverust_macros::post;
///
/// #[derive(Deserialize, Validate)]
/// struct CreateTask {
///     #[validate(length(min = 1, max = 200))]
///     title: String,
/// }
///
/// #[post("/tasks")]
/// async fn create(Json(task): Json<CreateTask>) -> &'static str {
///     // `task.title` já passou pela validação aqui
///     "created"
/// }
/// ```
///
/// `Json<T>` também implementa [`IntoResponse`] para `T: Serialize`, então o
/// mesmo tipo serve para entrada e saída do handler.
///
/// **Posicionamento na assinatura**: como `Json<T>` consome o body, ele tem
/// que ser o **último parâmetro** do handler. Extractors como `Path`, `Query`,
/// `State` (que só leem partes do request) vêm antes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let axum::Json(value) = axum::Json::<T>::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;

        match value.validate() {
            Ok(()) => Ok(Json(value)),
            Err(errors) => Err(validation_error_response(&errors)),
        }
    }
}

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}
