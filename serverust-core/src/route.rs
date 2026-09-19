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
    /// Se a rota dispensa autenticação.
    ///
    /// Default `false` — o default deny da ADR 0009. Com autenticação
    /// instalada, [`crate::App::route`] embrulha toda rota que **não** tenha
    /// isto ligado com o [`crate::AuthGate`]. Ligue via [`Route::public`].
    pub is_public: bool,
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
            is_public: false,
        }
    }

    /// Marca a rota como pública: ela responde mesmo sem identidade.
    ///
    /// É a exceção explícita ao default deny — o equivalente do `pub` do Rust
    /// para rotas. Marcar o que é aberto, em vez do que é protegido, é o que
    /// torna a superfície exposta auditável por presença: `grep` encontra
    /// marcação, nunca a falta dela.
    ///
    /// ```
    /// # use serverust_core::Route;
    /// # use axum::routing::get;
    /// # use utoipa::openapi::{HttpMethod, path::Operation};
    /// let route = Route::new("/health", HttpMethod::Get, get(|| async {}), Operation::new())
    ///     .public();
    /// assert!(route.is_public);
    /// ```
    pub fn public(mut self) -> Self {
        self.is_public = true;
        self
    }
}

/// Implementado pelas structs geradas pelas macros `#[get]`, `#[post]`, etc.
///
/// Permite passar o nome do handler diretamente para `App::route(handler)`.
pub trait IntoRoute {
    fn into_route(self) -> Route;
}
