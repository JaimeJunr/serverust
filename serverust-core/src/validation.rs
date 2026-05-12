use axum::extract::{FromRequest, Request};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error::validation_error_response;

/// Extractor JSON com validação automática.
///
/// Deserializa o body como `T` e, se a validação via `validator::Validate`
/// falhar, devolve HTTP 422 com payload padronizado antes mesmo do handler
/// ser invocado.
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
