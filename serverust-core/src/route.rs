use axum::routing::MethodRouter;
use utoipa::openapi::{HttpMethod, path::Operation};

use crate::container::Container;

/// Metadata de uma rota registrável no [`crate::App`].
///
/// O `MethodRouter` é parametrizado pelo [`Container`] (state do App) para
/// que handlers possam extrair serviços via `State<Arc<dyn Trait>>`.
pub struct Route {
    pub path: &'static str,
    pub method: HttpMethod,
    pub method_router: MethodRouter<Container>,
    pub operation: Operation,
}

impl Route {
    pub fn new(
        path: &'static str,
        method: HttpMethod,
        method_router: MethodRouter<Container>,
        operation: Operation,
    ) -> Self {
        Self {
            path,
            method,
            method_router,
            operation,
        }
    }
}

/// Implementado pelas structs geradas pelas macros `#[get]`, `#[post]`, etc.
///
/// Permite passar o nome do handler diretamente para `App::route(handler)`.
pub trait IntoRoute {
    fn into_route(self) -> Route;
}
