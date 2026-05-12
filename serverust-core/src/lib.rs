//! Core do framework **serverust** — APIs HTTP e AWS Lambda em Rust, com a
//! ergonomia do FastAPI e a arquitetura do NestJS.
//!
//! Este crate concentra as peças que você usa em todo handler:
//!
//! - [`App`] — builder principal: acumula rotas, registra services no
//!   [`Container`], expõe `/openapi.json`, `/docs` e `/redoc`.
//! - [`Route`] / [`IntoRoute`] — tipo emitido pelas macros `#[get]`/`#[post]`/etc
//!   (definidas em `serverust-macros`).
//! - [`extract::Json`] — extractor validante: roda `validator::Validate` antes
//!   do handler e responde HTTP 422 padronizado em falha.
//! - [`ApiError`] / [`validation_error_response`] — payload de erro JSON
//!   consistente. Use com `#[derive(ApiError)]` em enums de domínio.
//! - [`Guard`] / [`Pipe`] / [`Interceptor`] — primitivas de pipeline.
//! - [`ServerustConfig`] — config typed lida de `serverust.toml` via figment.
//!
//! Exemplo mínimo servindo HTTP local. Em produção, combine com o crate
//! `serverust-lambda` (trait `AppRuntime`) para um `.run()` que detecta entre
//! Lambda e HTTP automaticamente:
//!
//! ```no_run
//! use serverust_core::App;
//! use serverust_macros::get;
//!
//! #[get("/")]
//! async fn hello() -> &'static str { "hello" }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     App::new().route(hello).run_http("127.0.0.1:3000").await
//! }
//! ```
//!
//! Mais exemplos e tutorial completo em
//! <https://github.com/JaimeJunr/serverust/blob/main/docs/guides/lambda-tutorial.md>.

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
