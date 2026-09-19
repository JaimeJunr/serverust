//! Contrato de autenticação do framework — sem dependência de criptografia.
//!
//! `serverust-core` **não** implementa autenticação. Ele define apenas onde a
//! decisão "esta requisição pode seguir?" é tomada, e deixa outro crate dizer
//! quem é o requisitante. Isso mantém o core livre de cripto (ADR 0009) e faz
//! com que qualquer implementação de autenticação — inclusive uma escrita pelo
//! usuário — herde o default deny apenas inserindo os marcadores daqui.
//!
//! # Como as peças se encaixam
//!
//! ```text
//! Layer de autenticação (App::layer)      ← roda por FORA, em todas as rotas
//!   insere AuthEnabled sempre
//!   insere Authenticated se a credencial for válida
//!        ↓
//! AuthGate (App::route)                   ← roda por DENTRO, só em rota não-pública
//!   AuthEnabled presente e Authenticated ausente → 401, handler não executa
//!   caso contrário                               → segue
//! ```
//!
//! A ordem sai correta por construção: `App::layer` envolve o router inteiro,
//! enquanto o [`AuthGate`] embrulha o `MethodRouter` de cada rota. O layer de
//! fora sempre popula as extensions antes de o portão de dentro lê-las.
//!
//! Sem nenhum layer de autenticação instalado, o `AuthEnabled` nunca aparece e
//! o portão deixa tudo passar: quem não usa autenticação não muda de
//! comportamento.

use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use http::header::WWW_AUTHENTICATE;
use pin_project_lite::pin_project;
use tower::{Layer, Service};

/// Marcador de que **existe autenticação instalada** nesta aplicação.
///
/// Uma implementação de autenticação insere isto nas extensions de toda
/// requisição que passa por ela, independentemente de a credencial ser válida.
/// É o que liga o [`AuthGate`]: sem este marcador o portão é inerte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthEnabled;

/// Marcador de que **esta requisição traz identidade válida**.
///
/// Inserido pela implementação de autenticação somente quando a credencial foi
/// verificada com sucesso. O [`AuthGate`] não olha *quem* é o requisitante —
/// só se há identidade. Quem precisa dos dados usa o extractor tipado do crate
/// de autenticação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Authenticated;

/// Portão de rota que implementa o **default deny**: com autenticação
/// instalada, a rota só executa se a requisição estiver autenticada.
///
/// Aplicado automaticamente por [`crate::App::route`] a toda rota que não seja
/// marcada como pública via [`crate::Route::public`]. Raramente é usado
/// diretamente.
///
/// Rejeitar aqui, e não no layer de autenticação, é o que permite que rota
/// pública e rota esquecida sejam tratadas de forma diferente: o layer roda em
/// todas as rotas e não sabe qual delas casou, enquanto o portão só existe no
/// caminho das rotas que exigem identidade.
#[derive(Debug, Clone, Copy, Default)]
pub struct AuthGate;

impl<S> Layer<S> for AuthGate {
    type Service = AuthGateService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthGateService { inner }
    }
}

/// Service produzido por [`AuthGate`].
#[derive(Debug, Copy)]
pub struct AuthGateService<S> {
    inner: S,
}

// Clone manual: o derivado adicionaria bound desnecessário em cenários com
// `S` não-Clone, e queremos o mesmo requisito que o axum já impõe.
impl<S: Clone> Clone for AuthGateService<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<S> Service<Request> for AuthGateService<S>
where
    S: Service<Request, Response = Response>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = AuthGateFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let extensions = req.extensions();
        let auth_installed = extensions.get::<AuthEnabled>().is_some();
        let authenticated = extensions.get::<Authenticated>().is_some();

        if auth_installed && !authenticated {
            AuthGateFuture::Denied {
                response: Some(authentication_required()),
            }
        } else {
            AuthGateFuture::Allowed {
                inner: self.inner.call(req),
            }
        }
    }
}

pin_project! {
    /// Future de [`AuthGateService`]: encaminha para o handler ou devolve o 401
    /// já pronto.
    ///
    /// É um enum projetado com `pin-project-lite` em vez de um future boxado —
    /// o caminho permitido não paga alocação nem dispatch dinâmico.
    #[project = AuthGateFutureProj]
    pub enum AuthGateFuture<F> {
        Allowed { #[pin] inner: F },
        Denied { response: Option<Response> },
    }
}

impl<F, E> Future for AuthGateFuture<F>
where
    F: Future<Output = Result<Response, E>>,
{
    type Output = Result<Response, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            AuthGateFutureProj::Allowed { inner } => inner.poll(cx),
            AuthGateFutureProj::Denied { response } => Poll::Ready(Ok(response
                .take()
                .expect("AuthGateFuture::Denied pollado depois de concluído"))),
        }
    }
}

/// Resposta 401 do portão.
///
/// `WWW-Authenticate: Bearer` segue a RFC 6750 §3 — sem esse header o cliente
/// não sabe qual esquema usar.
///
/// O motivo é `authentication_required`, e não um código de falha de
/// credencial: o portão rejeita por **política da rota**, não por ter tentado
/// interpretar uma credencial. Quem distingue credencial ausente de inválida é
/// o extractor do crate de autenticação.
fn authentication_required() -> Response {
    // A ADR 0009 pede um 401 "que ensina" o desenvolvedor a anotar a rota. A
    // dica vai só em build de debug: em release ela seria vazamento de detalhe
    // interno para quem chama a API, que não pode fazer nada com ela.
    #[cfg(debug_assertions)]
    let body = serde_json::json!({
        "error": "unauthorized",
        "reason": "authentication_required",
        "hint": "rota não anotada: marque com #[public] se ela deve ser aberta",
    });

    #[cfg(not(debug_assertions))]
    let body = serde_json::json!({
        "error": "unauthorized",
        "reason": "authentication_required",
    });

    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, "Bearer")],
        axum::Json(body),
    )
        .into_response()
}
