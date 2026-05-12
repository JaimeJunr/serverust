//! Pipeline declarativa do serverust: Guards, Pipes e Interceptors.
//!
//! - [`Guard`]: verificação síncrona/async aplicada ANTES do handler. Macros
//!   `#[guard(...)]` injetam um [`GuardCheck`] como extractor no início da
//!   assinatura do handler, fazendo o axum rejeitar a requisição (401/403/etc.)
//!   sem chegar ao corpo do handler.
//! - [`Pipe<I>`]: transformação tipada de input → output. `ParseUuidPipe` é
//!   fornecido como exemplo canônico (`String` → `Uuid`). [`PipePath`] é o
//!   extractor que aplica o pipe sobre um segmento da URL.
//! - [`Interceptor`]: middleware tower-style aplicado via `App::interceptor()`.
//!   Envolve toda a execução (guards + pipes + handler), permitindo pré/pós
//!   processamento de request/response.

// As primitivas devolvem `Result<_, axum::response::Response>` por design —
// Response é a moeda de erro idiomática do axum. O `result_large_err` aqui é
// estrutural e não há ganho em boxar.
#![allow(clippy::result_large_err)]

use std::marker::PhantomData;

use axum::extract::FromRequestParts;
use axum::extract::Path;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use http::request::Parts;

// ---------------------------------------------------------------------------
// Guard
// ---------------------------------------------------------------------------

/// Verificação de autorização/permissão executada antes do handler.
///
/// Implementadores recebem a parte "leve" da requisição (`Parts` — sem body)
/// e devolvem `Ok(())` para permitir a execução ou um [`Response`] de
/// rejeição (tipicamente 401/403) para curto-circuitar.
pub trait Guard: Send + Sync + 'static {
    fn check(parts: &Parts) -> impl Future<Output = Result<(), Response>> + Send;
}

/// Extractor zero-cost que dispara a verificação do guard `G` como parte do
/// pipeline de extractors do axum. Usado pelas macros `#[guard(G)]`; raramente
/// instanciado diretamente.
pub struct GuardCheck<G: Guard>(PhantomData<fn() -> G>);

impl<S, G> FromRequestParts<S> for GuardCheck<G>
where
    G: Guard,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        G::check(parts).await?;
        Ok(Self(PhantomData))
    }
}

// ---------------------------------------------------------------------------
// Pipe
// ---------------------------------------------------------------------------

/// Transformação tipada aplicada a um input antes de chegar ao handler.
///
/// Pipes substituem código repetitivo de parsing/normalização. Em caso de
/// falha devolvem um [`Response`] (tipicamente 400) que curto-circuita a
/// execução do handler.
pub trait Pipe<I>: Send + Sync + 'static {
    type Output;

    fn transform(input: I) -> Result<Self::Output, Response>;
}

/// Extractor que aplica um [`Pipe<String>`] sobre um único segmento da URL
/// (extraído via `axum::extract::Path<String>`).
///
/// Exemplo: `PipePath<ParseUuidPipe>` em `/user/{id}` extrai o segmento como
/// `String` e o transforma em `Uuid`, devolvendo 400 se inválido.
pub struct PipePath<P>(pub <P as Pipe<String>>::Output)
where
    P: Pipe<String>;

impl<S, P> FromRequestParts<S> for PipePath<P>
where
    P: Pipe<String>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(raw): Path<String> = Path::from_request_parts(parts, state)
            .await
            .map_err(IntoResponse::into_response)?;
        let out = P::transform(raw)?;
        Ok(PipePath(out))
    }
}

/// Pipe que converte uma `String` em [`uuid::Uuid`]. Falha → HTTP 400 com
/// payload `{ "error": "invalid_uuid", "input": "<valor>" }`.
pub struct ParseUuidPipe;

impl Pipe<String> for ParseUuidPipe {
    type Output = uuid::Uuid;

    fn transform(input: String) -> Result<Self::Output, Response> {
        uuid::Uuid::parse_str(&input).map_err(|_| {
            let body = serde_json::json!({
                "error": "invalid_uuid",
                "input": input,
            });
            (StatusCode::BAD_REQUEST, axum::Json(body)).into_response()
        })
    }
}

// ---------------------------------------------------------------------------
// Interceptor
// ---------------------------------------------------------------------------

/// Middleware com semântica de "wrap" sobre a execução: pode inspecionar a
/// request antes do handler e/ou modificar a response depois.
///
/// Registrado via [`crate::App::interceptor`]. Internamente vira uma camada
/// `axum::middleware::from_fn`, aplicada apenas às rotas do usuário (não às
/// rotas de documentação geradas pelo App).
pub trait Interceptor: Send + Sync + 'static {
    fn intercept(&self, req: Request, next: Next) -> impl Future<Output = Response> + Send;
}
