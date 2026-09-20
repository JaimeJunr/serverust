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
    /// Escopos que a rota exige, para o `security` do OpenAPI.
    ///
    /// Preenchido pela macro `#[authorize]`. **Não é o que aplica a
    /// exigência** — quem faz isso é o `Guard` que a macro injeta. Aqui é só
    /// o que o documento precisa declarar, para que o botão "Authorize" do
    /// Scalar/Swagger UI peça os escopos certos.
    ///
    /// Separar as duas coisas é o que permite documentar sem mexer na trait
    /// [`crate::Guard`]; o custo é que um guard escrito à mão não aparece no
    /// documento, por não ter como declarar o que exige.
    pub required_scopes: &'static [&'static str],
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
            required_scopes: &[],
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

    /// Declara os escopos que a rota exige, para o `security` do OpenAPI.
    ///
    /// Chamada pela macro `#[authorize]`. Chamar à mão documenta a exigência
    /// sem aplicá-la — o que é pior do que não documentar, porque passa a
    /// mentir. Use junto com o guard que realmente verifica.
    pub fn scopes(mut self, scopes: &'static [&'static str]) -> Self {
        self.required_scopes = scopes;
        self
    }
}

/// Implementado pelas structs geradas pelas macros `#[get]`, `#[post]`, etc.
///
/// Permite passar o nome do handler diretamente para `App::route(handler)`.
pub trait IntoRoute {
    fn into_route(self) -> Route;
}
