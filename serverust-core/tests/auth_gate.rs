//! Testes do default deny por rota (ADR 0009, Emenda 1).
//!
//! O desenho inteiro depende de uma premissa de ordenação: o layer de
//! autenticação, aplicado por fora ao router, roda **antes** do `AuthGate`,
//! aplicado por dentro a cada rota. Se isso não valesse, o portão leria
//! extensions ainda vazias e negaria toda requisição, inclusive as
//! autenticadas. Há teste dedicado para essa premissa.

use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use http::{Method, Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serverust_core::{App, AuthEnabled, AuthFailure, Authenticated, Interceptor, IntoRoute, Route};
use serverust_macros::get as get_route;
use tower::ServiceExt;
use utoipa::openapi::HttpMethod;
use utoipa::openapi::path::Operation;

// ---------------------------------------------------------------------------
// Apoio
// ---------------------------------------------------------------------------

#[get_route("/privado")]
async fn privado() -> &'static str {
    "conteudo-privado"
}

/// Registra se o corpo do handler chegou a executar — prova que o portão
/// curto-circuita, e não apenas troca a resposta depois do handler.
///
/// O flag é global ao binário de teste, e os testes rodam em paralelo, então
/// esta rota é **exclusiva** do teste de curto-circuito. Compartilhá-la com
/// outro teste tornaria a asserção dependente da ordem de execução.
static HANDLER_EXECUTOU: AtomicBool = AtomicBool::new(false);

#[get_route("/privado-instrumentado")]
async fn privado_instrumentado() -> &'static str {
    HANDLER_EXECUTOU.store(true, Ordering::SeqCst);
    "conteudo-privado"
}

/// Rota pública construída pela via programática (`Route::public`), que é o
/// contrato estável enquanto a macro `#[public]` não existe.
struct RotaPublica;

impl IntoRoute for RotaPublica {
    fn into_route(self) -> Route {
        Route::new(
            "/health",
            HttpMethod::Get,
            get(|| async { "ok" }),
            Operation::new(),
        )
        .public()
    }
}

/// Simula um crate de autenticação: marca que há autenticação instalada e,
/// opcionalmente, que a requisição traz identidade válida.
struct AutenticacaoFalsa {
    identidade_valida: bool,
}

impl Interceptor for AutenticacaoFalsa {
    async fn intercept(&self, mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(AuthEnabled);
        if self.identidade_valida {
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
// Sem autenticação instalada, o portão é inerte
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sem_autenticacao_instalada_rota_nao_publica_responde_normalmente() {
    let router = App::new().without_docs().route(privado).into_router();

    let resp = router.oneshot(req("/privado")).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "quem não usa autenticação não pode mudar de comportamento"
    );
}

// ---------------------------------------------------------------------------
// Com autenticação instalada
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rota_nao_publica_sem_identidade_devolve_401() {
    let router = App::new()
        .without_docs()
        .interceptor(AutenticacaoFalsa {
            identidade_valida: false,
        })
        .route(privado)
        .into_router();

    let resp = router.oneshot(req("/privado")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Premissa de ordenação do desenho: o layer de fora popula as extensions
/// antes de o portão de dentro lê-las. Se a ordem fosse inversa, este teste
/// devolveria 401.
#[tokio::test]
async fn layer_externo_roda_antes_do_portao_interno() {
    let router = App::new()
        .without_docs()
        .interceptor(AutenticacaoFalsa {
            identidade_valida: true,
        })
        .route(privado)
        .into_router();

    let resp = router.oneshot(req("/privado")).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "o portão não enxergou o Authenticated que o layer externo inseriu"
    );
    assert_eq!(corpo(resp).await, "conteudo-privado");
}

#[tokio::test]
async fn ordem_do_builder_e_irrelevante() {
    // Autenticação configurada DEPOIS do registro da rota.
    let router = App::new()
        .without_docs()
        .route(privado)
        .interceptor(AutenticacaoFalsa {
            identidade_valida: false,
        })
        .into_router();

    let resp = router.oneshot(req("/privado")).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "o portão decide em runtime sobre um marcador, não na montagem"
    );
}

#[tokio::test]
async fn rota_publica_responde_sem_identidade() {
    let router = App::new()
        .without_docs()
        .interceptor(AutenticacaoFalsa {
            identidade_valida: false,
        })
        .route(RotaPublica)
        .into_router();

    let resp = router.oneshot(req("/health")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(corpo(resp).await, "ok");
}

// ---------------------------------------------------------------------------
// O portão curto-circuita: o handler não executa
// ---------------------------------------------------------------------------

#[tokio::test]
async fn portao_impede_o_handler_de_executar() {
    let router = App::new()
        .without_docs()
        .interceptor(AutenticacaoFalsa {
            identidade_valida: false,
        })
        .route(privado_instrumentado)
        .into_router();

    let resp = router.oneshot(req("/privado-instrumentado")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(
        !HANDLER_EXECUTOU.load(Ordering::SeqCst),
        "o corpo do handler executou apesar do 401 — o portão trocou a \
         resposta em vez de curto-circuitar"
    );
}

// ---------------------------------------------------------------------------
// Forma da resposta de rejeição
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rejeicao_traz_www_authenticate_e_motivo_estavel() {
    let router = App::new()
        .without_docs()
        .interceptor(AutenticacaoFalsa {
            identidade_valida: false,
        })
        .route(privado)
        .into_router();

    let resp = router.oneshot(req("/privado")).await.unwrap();

    assert_eq!(
        resp.headers().get(http::header::WWW_AUTHENTICATE).unwrap(),
        "Bearer",
        "401 sem WWW-Authenticate não diz ao cliente qual esquema usar (RFC 6750 §3)"
    );

    let corpo = corpo(resp).await;
    assert!(
        corpo.contains(r#""error":"unauthorized""#),
        "corpo: {corpo}"
    );
    assert!(
        corpo.contains(r#""reason":"authentication_required""#),
        "corpo: {corpo}"
    );
}

/// Quando a implementação registra um motivo preciso, o portão o repassa em
/// vez do genérico. Sem isto, proteger uma rota pelo default deny apagaria
/// códigos úteis como `token_expired`, porque o portão rejeita antes de
/// qualquer extractor rodar.
#[tokio::test]
async fn portao_repassa_o_motivo_registrado_pela_implementacao() {
    struct FalhaComMotivo;

    impl Interceptor for FalhaComMotivo {
        async fn intercept(&self, mut req: Request, next: Next) -> Response {
            req.extensions_mut().insert(AuthEnabled);
            req.extensions_mut().insert(AuthFailure("token_expired"));
            next.run(req).await
        }
    }

    let router = App::new()
        .without_docs()
        .interceptor(FalhaComMotivo)
        .route(privado)
        .into_router();

    let resp = router.oneshot(req("/privado")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let corpo = corpo(resp).await;
    assert!(
        corpo.contains(r#""reason":"token_expired""#),
        "o portão substituiu o motivo preciso pelo genérico — corpo: {corpo}"
    );
}

/// Sem motivo registrado, a rejeição é genérica: política de rota, sem
/// alegar nada sobre a credencial.
#[tokio::test]
async fn sem_motivo_registrado_o_portao_usa_o_generico() {
    let router = App::new()
        .without_docs()
        .interceptor(AutenticacaoFalsa {
            identidade_valida: false,
        })
        .route(privado)
        .into_router();

    let corpo = corpo(router.oneshot(req("/privado")).await.unwrap()).await;

    assert!(
        corpo.contains(r#""reason":"authentication_required""#),
        "corpo: {corpo}"
    );
}

/// A dica para o desenvolvedor só pode existir em build de debug: em release
/// ela seria vazamento de detalhe interno para quem chama a API.
#[tokio::test]
async fn dica_de_anotacao_nao_vaza_em_release() {
    let router = App::new()
        .without_docs()
        .interceptor(AutenticacaoFalsa {
            identidade_valida: false,
        })
        .route(privado)
        .into_router();

    let corpo = corpo(router.oneshot(req("/privado")).await.unwrap()).await;

    if cfg!(debug_assertions) {
        assert!(corpo.contains("#[public]"), "corpo: {corpo}");
    } else {
        assert!(!corpo.contains("#[public]"), "corpo: {corpo}");
        assert!(!corpo.contains("hint"), "corpo: {corpo}");
    }
}

// ---------------------------------------------------------------------------
// Rotas de documentação
// ---------------------------------------------------------------------------

/// As rotas de documentação não passam por `App::route`, então não recebem o
/// portão — seguem acessíveis mesmo sob default deny. Quem precisa fechá-las
/// usa `App::without_docs()`.
#[tokio::test]
async fn rotas_de_documentacao_seguem_abertas_sob_default_deny() {
    let router = App::new()
        .interceptor(AutenticacaoFalsa {
            identidade_valida: false,
        })
        .route(privado)
        .into_router();

    let resp = router.oneshot(req("/openapi.json")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Builder de Route
// ---------------------------------------------------------------------------

#[test]
fn route_nasce_nao_publica_e_public_liga_o_flag() {
    let rota = Route::new("/x", HttpMethod::Get, get(|| async {}), Operation::new());
    assert!(!rota.is_public, "o default é negar, não abrir");

    assert!(rota.public().is_public);
}
