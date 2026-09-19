//! Tower layer que autentica a requisição uma única vez.

use std::marker::PhantomData;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::Request;
use tower::{Layer, Service};

use crate::claims::{AuthzFacts, Claims};
use crate::jwt::{JwtAuth, bearer_token};

/// Layer que valida o token da requisição e deposita o resultado nas
/// extensions, para que extractors e guards o leiam sem revalidar.
///
/// **O layer não rejeita requisição.** Ele apenas registra o que encontrou:
/// as claims, se o token era válido, ou o [`AuthError`] correspondente. Quem
/// transforma isso em 401 é o extractor [`crate::Auth`] — ou, no futuro, o
/// guard de rota. Essa separação existe porque o layer roda em todas as rotas,
/// inclusive nas públicas, e rejeitar ali impediria uma rota pública de
/// responder.
///
/// Registre com `App::layer`:
///
/// ```no_run
/// use serverust_auth::{AuthLayer, JwtAuth, StandardClaims};
/// use serverust_core::App;
///
/// let auth = JwtAuth::hs256(b"segredo");
/// let app = App::new().layer(AuthLayer::<StandardClaims>::new(auth));
/// ```
pub struct AuthLayer<C> {
    auth: Arc<JwtAuth>,
    _claims: PhantomData<fn() -> C>,
}

impl<C> AuthLayer<C> {
    /// Cria o layer tomando posse do verificador.
    pub fn new(auth: JwtAuth) -> Self {
        Self::shared(Arc::new(auth))
    }

    /// Cria o layer a partir de um verificador já compartilhado — útil quando
    /// o mesmo `JwtAuth` alimenta mais de um ponto da aplicação.
    pub fn shared(auth: Arc<JwtAuth>) -> Self {
        Self {
            auth,
            _claims: PhantomData,
        }
    }
}

// Clone manual: o derivado exigiria `C: Clone` sem necessidade — `C` só
// aparece em `PhantomData`.
impl<C> Clone for AuthLayer<C> {
    fn clone(&self) -> Self {
        Self {
            auth: Arc::clone(&self.auth),
            _claims: PhantomData,
        }
    }
}

impl<S, C> Layer<S> for AuthLayer<C> {
    type Service = AuthService<S, C>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            auth: Arc::clone(&self.auth),
            _claims: PhantomData,
        }
    }
}

/// Service produzido por [`AuthLayer`].
pub struct AuthService<S, C> {
    inner: S,
    auth: Arc<JwtAuth>,
    _claims: PhantomData<fn() -> C>,
}

impl<S: Clone, C> Clone for AuthService<S, C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            auth: Arc::clone(&self.auth),
            _claims: PhantomData,
        }
    }
}

impl<S, C> Service<Request> for AuthService<S, C>
where
    S: Service<Request>,
    C: Claims,
{
    type Response = S::Response;
    type Error = S::Error;
    // A verificação com chave estática é síncrona e não falha o service, então
    // o future do inner passa direto: sem box, sem pin-project, sem alocação.
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        match bearer_token(req.headers()).and_then(|token| self.auth.verify::<C>(token)) {
            Ok(claims) => {
                let claims = Arc::new(claims);
                // Duas entradas apontando para a MESMA alocação: a tipada,
                // consumida por `Auth<C>`, e a apagada de tipo, consumida por
                // guards genéricos que não conhecem `C`. A coerção de
                // `Arc<C>` para `Arc<dyn AuthzFacts>` não copia nada.
                let facts: Arc<dyn AuthzFacts> = claims.clone();
                req.extensions_mut().insert(claims);
                req.extensions_mut().insert(facts);
            }
            Err(err) => {
                req.extensions_mut().insert(err);
            }
        }

        self.inner.call(req)
    }
}
