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
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use http::header::WWW_AUTHENTICATE;
use http::request::Parts;
use pin_project_lite::pin_project;
use tower::{Layer, Service};

/// Visão de autorização sobre a identidade da requisição, apagada de tipo.
///
/// Vive no core, e não no crate de autenticação, porque é **contrato** e não
/// implementação: não toca cripto, não sabe o que é um JWT, e é o que permite
/// que a macro `#[authorize]` gere código sem depender de qual crate
/// autenticou a requisição.
///
/// A implementação de autenticação publica um `Arc<dyn AuthzFacts>` nas
/// extensions; os guards de autorização leem de lá.
///
/// `has_scope` e `has_role` têm default `false`: um tipo de claims que só
/// carrega identidade nega toda autorização em vez de concedê-la por omissão.
pub trait AuthzFacts: Send + Sync + 'static {
    /// Identificador do principal — tipicamente a claim `sub`.
    fn subject(&self) -> &str;

    /// Se o principal possui o escopo informado.
    fn has_scope(&self, _scope: &str) -> bool {
        false
    }

    /// Se o principal possui o papel informado.
    fn has_role(&self, _role: &str) -> bool {
        false
    }
}

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

/// Motivo estável pelo qual a autenticação falhou, registrado pela
/// implementação para que o [`AuthGate`] o repasse ao cliente.
///
/// Sem isto o portão só saberia dizer "faltou identidade", e códigos úteis
/// como `token_expired` — que dizem ao cliente para renovar em vez de
/// reautenticar — se perderiam, porque o portão rejeita **antes** de qualquer
/// extractor rodar.
///
/// O conteúdo é `&'static str` de propósito: o core não interpreta credencial
/// e não deve alocar no caminho de rejeição.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthFailure(pub &'static str);

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
            // Repassa o motivo preciso quando a implementação registrou um;
            // sem ele, a rejeição é genérica por política de rota.
            let reason = extensions
                .get::<AuthFailure>()
                .map_or("authentication_required", |f| f.0);

            AuthGateFuture::Denied {
                response: Some(unauthorized(reason)),
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

/// Resposta 401 do portão, com o motivo estável informado.
///
/// `WWW-Authenticate: Bearer` segue a RFC 6750 §3 — sem esse header o cliente
/// não sabe qual esquema usar.
///
/// O motivo vem do [`AuthFailure`] quando a implementação registrou um (e aí
/// carrega a precisão dela: `token_expired`, `invalid_issuer`, ...); na
/// ausência dele é `authentication_required`, que significa rejeição por
/// política da rota, sem tentativa de interpretar credencial.
fn unauthorized(reason: &'static str) -> Response {
    // A ADR 0009 pede um 401 "que ensina" o desenvolvedor a anotar a rota. A
    // dica vai só em build de debug: em release ela seria vazamento de detalhe
    // interno para quem chama a API, que não pode fazer nada com ela.
    #[cfg(debug_assertions)]
    let body = serde_json::json!({
        "error": "unauthorized",
        "reason": reason,
        "hint": "rota não anotada: marque com #[public] se ela deve ser aberta",
    });

    #[cfg(not(debug_assertions))]
    let body = serde_json::json!({
        "error": "unauthorized",
        "reason": reason,
    });

    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, "Bearer")],
        axum::Json(body),
    )
        .into_response()
}

/// Verificação de autorização gerada pela macro `#[authorize]`.
///
/// Fica no core, e não na macro, por dois motivos: o código emitido no crate
/// do usuário encolhe para uma chamada, e a política de rejeição passa a ter
/// um lugar só — corrigi-la não exige recompilar quem já gerou o guard com
/// uma versão antiga da macro.
///
/// Semântica: **todos** os escopos e **todos** os papéis listados são
/// exigidos (AND). Empilhar `#[authorize]` também conjunta. Exigência
/// alternativa (OR) não é expressável hoje, de propósito: um `any_of`
/// ambíguo entre "qualquer um destes" e "qualquer um de tudo" é exatamente o
/// tipo de default que a ADR 0009 pede para não existir.
///
/// Falha fechado em duas frentes:
///
/// - **Sem fatos nas extensions** — seja porque não há autenticação
///   instalada, seja porque a rota é pública — a resposta é 401. Uma rota que
///   pede autorização e não tem de onde extrair identidade não pode executar.
/// - **Fatos presentes sem o escopo/papel** — 403 `insufficient_scope`,
///   o código da RFC 6750 §3.1.
#[doc(hidden)]
// O `Result<_, Response>` é a assinatura do `Guard::check`, que esta função
// alimenta: boxar aqui só adicionaria uma alocação no caminho de rejeição
// para desboxar em seguida.
#[allow(clippy::result_large_err)]
pub fn check_authz(parts: &Parts, scopes: &[&str], roles: &[&str]) -> Result<(), Response> {
    let Some(facts) = parts.extensions.get::<Arc<dyn AuthzFacts>>() else {
        return Err(unauthorized("authentication_required"));
    };

    if scopes.iter().all(|s| facts.has_scope(s)) && roles.iter().all(|r| facts.has_role(r)) {
        return Ok(());
    }

    Err(forbidden("insufficient_scope"))
}

/// Resposta 403 de autorização negada.
///
/// O corpo espelha o do 401 (`error` + `reason` estável) para que o cliente
/// ramifique da mesma forma nos dois casos. Sem `hint` mesmo em debug: aqui a
/// rota **está** anotada, e o que falta é permissão do principal — não há
/// anotação a sugerir ao desenvolvedor.
fn forbidden(reason: &'static str) -> Response {
    (
        StatusCode::FORBIDDEN,
        [(WWW_AUTHENTICATE, "Bearer")],
        axum::Json(serde_json::json!({ "error": "forbidden", "reason": reason })),
    )
        .into_response()
}
