//! Tower layer que autentica a requisição uma única vez.

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::Request;
use pin_project_lite::pin_project;
use serverust_core::{AuthEnabled, AuthFailure, Authenticated};
use tower::{Layer, Service};

use crate::claims::{AuthzFacts, Claims};
use crate::jwt::{JwtAuth, bearer_token};
use crate::verifier::Verifier;

/// Layer que valida o token da requisição e deposita o resultado nas
/// extensions, para que extractors e guards o leiam sem revalidar.
///
/// **O layer não rejeita requisição.** Ele apenas registra o que encontrou:
/// as claims, se o token era válido, ou o [`AuthError`] correspondente. Quem
/// transforma isso em 401 é o `AuthGate` do core — que `App::route` aplica a
/// toda rota não marcada como pública — ou o extractor [`crate::Auth`]. Essa
/// separação existe porque o layer roda em todas as rotas, inclusive nas
/// públicas, e rejeitar ali impediria uma rota pública de responder.
///
/// Instalar este layer **ativa o default deny**: a partir daí, rota que não
/// seja `Route::public()` exige identidade válida. É o efeito pretendido, e é
/// mudança de comportamento para quem já usava o layer contando apenas com o
/// extractor.
///
/// Registre com `App::auth`:
///
/// ```no_run
/// use serverust_auth::{AuthLayer, JwtAuth, StandardClaims};
/// use serverust_core::App;
///
/// let auth = JwtAuth::hs256(b"segredo");
/// let app = App::new().auth(AuthLayer::<StandardClaims>::new(auth));
/// ```
///
/// O parâmetro `V` é a origem das chaves, e tem default [`JwtAuth`] para que
/// `AuthLayer::<C>` continue significando o que sempre significou. Com outro
/// verificador — [`crate::JwksAuth`], ou um escrito por você — deixe a
/// inferência resolver:
///
/// ```text
/// let auth = JwksAuth::discover("https://idp.exemplo.com/").await?;
/// let app = App::new().auth(AuthLayer::<StandardClaims, _>::new(auth));
/// ```
pub struct AuthLayer<C, V = JwtAuth> {
    auth: Arc<V>,
    _claims: PhantomData<fn() -> C>,
}

impl<C, V> AuthLayer<C, V> {
    /// Cria o layer tomando posse do verificador.
    pub fn new(auth: V) -> Self {
        Self::shared(Arc::new(auth))
    }

    /// Cria o layer a partir de um verificador já compartilhado — útil quando
    /// o mesmo verificador alimenta mais de um ponto da aplicação.
    pub fn shared(auth: Arc<V>) -> Self {
        Self {
            auth,
            _claims: PhantomData,
        }
    }
}

// Clone manual: o derivado exigiria `C: Clone` e `V: Clone` sem necessidade —
// `C` só aparece em `PhantomData` e `V` vive atrás de `Arc`.
impl<C, V> Clone for AuthLayer<C, V> {
    fn clone(&self) -> Self {
        Self {
            auth: Arc::clone(&self.auth),
            _claims: PhantomData,
        }
    }
}

impl<S, C, V> Layer<S> for AuthLayer<C, V> {
    type Service = AuthService<S, C, V>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            auth: Arc::clone(&self.auth),
            _claims: PhantomData,
        }
    }
}

/// Service produzido por [`AuthLayer`].
pub struct AuthService<S, C, V = JwtAuth> {
    inner: S,
    auth: Arc<V>,
    _claims: PhantomData<fn() -> C>,
}

impl<S: Clone, C, V> Clone for AuthService<S, C, V> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            auth: Arc::clone(&self.auth),
            _claims: PhantomData,
        }
    }
}

impl<S, C, V> Service<Request> for AuthService<S, C, V>
where
    S: Service<Request> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Response: 'static,
    S::Error: 'static,
    C: Claims,
    V: Verifier,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = AuthFuture<S::Future, Self::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        // Liga o default deny do core, independentemente de a credencial ser
        // válida: o `AuthGate` precisa distinguir "não há autenticação nesta
        // aplicação" de "há autenticação e esta requisição não passou".
        // Inserir isto ANTES da verificação garante que o marcador exista
        // mesmo quando o token é inválido — é o caso em que negar importa.
        req.extensions_mut().insert(AuthEnabled);

        let resultado = bearer_token(req.headers()).and_then(|token| self.auth.verify::<C>(token));

        // O caminho lento existe para um caso só: o emissor rotacionou as
        // chaves depois de este processo ter carregado o JWKS. `can_revalidate`
        // é uma comparação de enum, e devolve `false` para todo verificador de
        // chave estática — então nem o `Box` nem o `clone` do inner service
        // abaixo chegam a existir para quem não usa JWKS.
        match resultado {
            Err(err) if self.auth.can_revalidate(&err) => self.revalidar(req, err),
            outro => AuthFuture::Direto {
                inner: self.inner.call(depositar(req, outro)),
            },
        }
    }
}

impl<S, C, V> AuthService<S, C, V>
where
    S: Service<Request> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Response: 'static,
    S::Error: 'static,
    C: Claims,
    V: Verifier,
{
    /// Caminho lento: rebusca material de chave e tenta uma segunda vez.
    ///
    /// Boxa o encadeamento inteiro — revalidar, depositar o resultado, chamar
    /// o inner — em vez de modelar cada passo como variante do future. Uma
    /// alocação aqui não custa nada perto da ida à rede que ela acompanha, e
    /// mantém o caminho quente com uma variante só.
    fn revalidar(
        &mut self,
        req: Request,
        err: crate::error::AuthError,
    ) -> AuthFuture<S::Future, S::Response, S::Error> {
        let auth = Arc::clone(&self.auth);
        // O contrato do tower diz que `poll_ready` reserva capacidade para UMA
        // chamada. Clonar o service e chamar o clone — deixando o original com
        // a reserva — é o padrão que o próprio tower documenta para quando a
        // chamada não acontece já.
        let mut inner = self.inner.clone();
        std::mem::swap(&mut inner, &mut self.inner);

        let token = bearer_token(req.headers()).map(str::to_owned);

        AuthFuture::Lento {
            fut: Box::pin(async move {
                let resultado = match token {
                    Ok(token) => auth.revalidate::<C>(&token, err).await,
                    // Não chega aqui: sem token o erro é `Missing`, e nenhum
                    // verificador revalida sobre ele.
                    Err(e) => Err(e),
                };

                inner.call(depositar(req, resultado)).await
            }),
        }
    }
}

/// Deposita o resultado da verificação nas extensions da requisição.
///
/// Em função separada porque os dois caminhos — rápido e lento — precisam
/// depositar exatamente a mesma coisa. Duplicar isto seria abrir espaço para
/// uma metade inserir `Authenticated` e a outra não.
fn depositar<C: Claims>(
    mut req: Request,
    resultado: Result<C, crate::error::AuthError>,
) -> Request {
    match resultado {
        Ok(claims) => {
            let claims = Arc::new(claims);
            // Duas entradas apontando para a MESMA alocação: a tipada,
            // consumida por `Auth<C>`, e a apagada de tipo, consumida por
            // guards genéricos que não conhecem `C`. A coerção de `Arc<C>`
            // para `Arc<dyn AuthzFacts>` não copia nada.
            let facts: Arc<dyn AuthzFacts> = claims.clone();
            req.extensions_mut().insert(claims);
            req.extensions_mut().insert(facts);
            // Marcador apagado de tipo que o `AuthGate` lê. Só aqui: o ramo de
            // erro não pode inseri-lo, sob pena de o portão liberar requisição
            // com token inválido.
            req.extensions_mut().insert(Authenticated);
        }
        Err(err) => {
            // O portão rejeita antes de qualquer extractor rodar, então o
            // motivo preciso só chega ao cliente se for registrado aqui.
            req.extensions_mut().insert(AuthFailure(err.reason()));
            req.extensions_mut().insert(err);
        }
    }
    req
}

pin_project! {
    /// Future do [`AuthService`].
    ///
    /// Duas variantes, e a assimetria entre elas é o ponto: `Direto` repassa o
    /// future do inner service sem box nem alocação, e é por onde passa
    /// essencialmente toda requisição. `Lento` só é construída quando o
    /// verificador disse que vale rebuscar material de chave.
    #[project = AuthFutureProj]
    pub enum AuthFuture<F, R, E> {
        Direto { #[pin] inner: F },
        Lento { #[pin] fut: Pin<Box<dyn Future<Output = Result<R, E>> + Send>> },
    }
}

impl<F, R, E> Future for AuthFuture<F, R, E>
where
    F: Future<Output = Result<R, E>>,
{
    type Output = Result<R, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            AuthFutureProj::Direto { inner } => inner.poll(cx),
            AuthFutureProj::Lento { fut } => fut.poll(cx),
        }
    }
}
