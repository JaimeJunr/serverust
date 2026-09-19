//! `#[public]` e `#[authorize]` (ADR 0009, etapa 2).
//!
//! Os testes montam um `App` real com um interceptor que faz o papel do crate
//! de autenticação: insere os marcadores do contrato (`AuthEnabled`,
//! `Authenticated`) e os fatos de autorização. Assim a cobertura fica sobre o
//! que as macros produzem, sem arrastar cripto para cá.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use http::{Method, Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serverust_core::{App, AuthEnabled, Authenticated, AuthzFacts, Interceptor};
use serverust_macros::{authorize, get, public};
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Apoio
// ---------------------------------------------------------------------------

/// Principal de teste. É o que um crate de autenticação publicaria nas
/// extensions depois de verificar a credencial.
struct Principal {
    escopos: Vec<&'static str>,
    papeis: Vec<&'static str>,
}

impl AuthzFacts for Principal {
    fn subject(&self) -> &str {
        "user-42"
    }

    fn has_scope(&self, scope: &str) -> bool {
        self.escopos.contains(&scope)
    }

    fn has_role(&self, role: &str) -> bool {
        self.papeis.contains(&role)
    }
}

/// Faz o papel do `AuthLayer`: liga o default deny e, quando há identidade,
/// publica os fatos.
struct AutenticacaoFalsa {
    principal: Option<(Vec<&'static str>, Vec<&'static str>)>,
}

impl AutenticacaoFalsa {
    fn anonima() -> Self {
        Self { principal: None }
    }

    fn com(escopos: &[&'static str], papeis: &[&'static str]) -> Self {
        Self {
            principal: Some((escopos.to_vec(), papeis.to_vec())),
        }
    }
}

impl Interceptor for AutenticacaoFalsa {
    async fn intercept(&self, mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(AuthEnabled);
        if let Some((escopos, papeis)) = &self.principal {
            let fatos: Arc<dyn AuthzFacts> = Arc::new(Principal {
                escopos: escopos.clone(),
                papeis: papeis.clone(),
            });
            req.extensions_mut().insert(fatos);
            req.extensions_mut().insert(Authenticated);
        }
        next.run(req).await
    }
}

fn req(path: &str) -> HttpRequest<Body> {
    HttpRequest::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

async fn corpo(resp: Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

// ---------------------------------------------------------------------------
// Rotas
// ---------------------------------------------------------------------------

#[public]
#[get("/health")]
async fn health() -> &'static str {
    "ok"
}

#[get("/relatorio")]
async fn relatorio() -> &'static str {
    "confidencial"
}

#[authorize(scope = "orders:read")]
#[get("/pedidos")]
async fn pedidos() -> &'static str {
    "pedidos"
}

/// Dois requisitos numa anotação só: conjunção.
#[authorize(scope = "orders:read", role = "admin")]
#[get("/pedidos/admin")]
async fn pedidos_admin() -> &'static str {
    "admin"
}

/// Anotações empilhadas também conjuntam — e provam que os parâmetros
/// injetados não colidem entre si.
#[authorize(scope = "a")]
#[authorize(scope = "b")]
#[get("/dois-escopos")]
async fn dois_escopos() -> &'static str {
    "ambos"
}

/// `#[authorize]` **abaixo** da macro de rota: o guard vira um item dentro do
/// corpo de `into_route`, mas a verificação é a mesma.
#[get("/abaixo")]
#[authorize(scope = "orders:read")]
async fn abaixo() -> &'static str {
    "abaixo"
}

fn app(auth: AutenticacaoFalsa) -> axum::Router {
    App::new()
        .without_docs()
        .interceptor(auth)
        .route(health)
        .route(relatorio)
        .route(pedidos)
        .route(pedidos_admin)
        .route(dois_escopos)
        .route(abaixo)
        .into_router()
}

// ---------------------------------------------------------------------------
// #[public]
// ---------------------------------------------------------------------------

#[tokio::test]
async fn public_abre_a_rota_declarada_por_macro() {
    let resp = app(AutenticacaoFalsa::anonima())
        .oneshot(req("/health"))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "#[public] não chegou à Route — a rota continuou sob o default deny"
    );
    assert_eq!(corpo(resp).await, "ok");
}

#[tokio::test]
async fn rota_sem_public_continua_negada() {
    let resp = app(AutenticacaoFalsa::anonima())
        .oneshot(req("/relatorio"))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "#[public] vazou para uma rota que não o tem"
    );
}

#[tokio::test]
async fn public_nao_muda_o_caminho_autenticado() {
    let resp = app(AutenticacaoFalsa::com(&[], &[]))
        .oneshot(req("/health"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// #[authorize]
// ---------------------------------------------------------------------------

#[tokio::test]
async fn authorize_permite_com_o_escopo() {
    let resp = app(AutenticacaoFalsa::com(&["orders:read"], &[]))
        .oneshot(req("/pedidos"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(corpo(resp).await, "pedidos");
}

#[tokio::test]
async fn authorize_nega_com_403_sem_o_escopo() {
    let resp = app(AutenticacaoFalsa::com(&["outro:escopo"], &[]))
        .oneshot(req("/pedidos"))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "autenticado sem permissão é 403, não 401 — 401 diria ao cliente para \
         reautenticar, e reautenticar não resolveria"
    );

    let corpo = corpo(resp).await;
    assert!(corpo.contains(r#""error":"forbidden""#), "corpo: {corpo}");
    assert!(
        corpo.contains(r#""reason":"insufficient_scope""#),
        "corpo: {corpo}"
    );
}

#[tokio::test]
async fn authorize_exige_escopo_e_papel_juntos() {
    let so_escopo = app(AutenticacaoFalsa::com(&["orders:read"], &[]))
        .oneshot(req("/pedidos/admin"))
        .await
        .unwrap();
    assert_eq!(so_escopo.status(), StatusCode::FORBIDDEN);

    let so_papel = app(AutenticacaoFalsa::com(&[], &["admin"]))
        .oneshot(req("/pedidos/admin"))
        .await
        .unwrap();
    assert_eq!(so_papel.status(), StatusCode::FORBIDDEN);

    let ambos = app(AutenticacaoFalsa::com(&["orders:read"], &["admin"]))
        .oneshot(req("/pedidos/admin"))
        .await
        .unwrap();
    assert_eq!(ambos.status(), StatusCode::OK);
}

#[tokio::test]
async fn authorize_empilhado_conjunta() {
    let so_um = app(AutenticacaoFalsa::com(&["a"], &[]))
        .oneshot(req("/dois-escopos"))
        .await
        .unwrap();
    assert_eq!(
        so_um.status(),
        StatusCode::FORBIDDEN,
        "empilhar #[authorize] virou OR — um escopo bastou"
    );

    let ambos = app(AutenticacaoFalsa::com(&["a", "b"], &[]))
        .oneshot(req("/dois-escopos"))
        .await
        .unwrap();
    assert_eq!(ambos.status(), StatusCode::OK);
}

#[tokio::test]
async fn authorize_funciona_abaixo_da_macro_de_rota() {
    let negado = app(AutenticacaoFalsa::com(&[], &[]))
        .oneshot(req("/abaixo"))
        .await
        .unwrap();
    assert_eq!(negado.status(), StatusCode::FORBIDDEN);

    let permitido = app(AutenticacaoFalsa::com(&["orders:read"], &[]))
        .oneshot(req("/abaixo"))
        .await
        .unwrap();
    assert_eq!(permitido.status(), StatusCode::OK);
}

/// Nem mesmo sem autenticação instalada: um `#[authorize]` que passa porque
/// ninguém configurou o layer seria o pior default possível.
#[tokio::test]
async fn authorize_nega_sem_autenticacao_instalada() {
    let router = App::new().without_docs().route(pedidos).into_router();

    let resp = router.oneshot(req("/pedidos")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
