//! Extractors que entregam a identidade ao handler.

use std::convert::Infallible;
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::response::Response;
use http::request::Parts;

use crate::claims::Claims;
use crate::error::{AuthError, unauthorized};

/// Identidade autenticada, exigida pelo handler.
///
/// Pedir `Auth<C>` na assinatura é o que protege a rota: sem token válido o
/// extractor devolve 401 e **o corpo do handler não executa**. Não há como
/// ler a identidade sem passar pela verificação.
///
/// ```no_run
/// use serverust_auth::{Auth, StandardClaims};
/// use serverust_macros::get;
///
/// #[get("/me")]
/// async fn me(user: Auth<StandardClaims>) -> String {
///     format!("olá, {}", user.sub)
/// }
/// ```
///
/// Se o [`crate::AuthLayer`] não estiver instalado, o extractor nega: uma rota
/// que pede identidade sem autenticação configurada falha fechada.
pub struct Auth<C>(pub Arc<C>);

impl<C> Clone for Auth<C> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<C> Deref for Auth<C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.0
    }
}

impl<S, C> FromRequestParts<S> for Auth<C>
where
    C: Claims,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(claims) = parts.extensions.get::<Arc<C>>() {
            return Ok(Self(Arc::clone(claims)));
        }

        // O layer registra o motivo exato da falha; sua ausência significa que
        // o layer não rodou, e aí a credencial simplesmente não existe.
        let err = parts
            .extensions
            .get::<AuthError>()
            .cloned()
            .unwrap_or(AuthError::Missing);

        Err(unauthorized(&err))
    }
}

/// Identidade quando houver, sem exigir que haja.
///
/// Para rotas que mudam de comportamento com usuário autenticado mas seguem
/// respondendo sem ele. Nunca rejeita.
pub struct MaybeAuth<C>(pub Option<Arc<C>>);

impl<C> Clone for MaybeAuth<C> {
    fn clone(&self) -> Self {
        Self(self.0.as_ref().map(Arc::clone))
    }
}

impl<S, C> FromRequestParts<S> for MaybeAuth<C>
where
    C: Claims,
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<Arc<C>>().map(Arc::clone)))
    }
}
