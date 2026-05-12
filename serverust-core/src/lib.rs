//! Core do framework serverust: App builder, Route, validação e erros.

mod app;
pub mod config;
mod container;
mod error;
mod openapi;
mod pipeline;
mod route;
mod validation;

pub use app::App;
pub use config::ServerustConfig;
pub use container::{Container, Injectable};
pub use error::{ApiError, validation_error_response};
pub use pipeline::{Guard, GuardCheck, Interceptor, ParseUuidPipe, Pipe, PipePath};
pub use route::{IntoRoute, Route};
pub use validation::Json;

/// Extractors tipados expostos pelo framework.
///
/// `Json` é a versão validada do serverust (executa `Validate` antes do handler);
/// os demais são re-exports diretos do axum.
pub mod extract {
    pub use crate::validation::Json;
    pub use axum::extract::{Path, Query, State};
}

/// Itens internos usados pelas macros geradas. Não fazem parte da API pública estável.
#[doc(hidden)]
pub mod __private {
    pub use axum;
    pub use http;
    pub use serde_json;
    pub use utoipa;
}
