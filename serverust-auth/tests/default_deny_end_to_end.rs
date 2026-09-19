//! Default deny de ponta a ponta, com o `AuthLayer` REAL.
//!
//! Os demais testes cobrem cada metade do contrato isoladamente: nos testes do
//! `serverust-core`, um interceptor escrito à mão insere os marcadores que o
//! `AuthGate` lê. Isso deixava um vão — nada provava que o `AuthLayer` de
//! verdade insere esses marcadores, e durante um tempo ele não inseria: o
//! portão existia, a documentação anunciava default deny, e na prática toda
//! rota passava.
//!
//! Estes testes fecham o vão montando um `App` com o layer real e tokens
//! reais, sem nenhum dublê.

mod common;

use axum::routing::get;
use http::StatusCode;
use serde_json::json;
use serverust_auth::{Auth, AuthLayer, JwtAuth, StandardClaims};
use serverust_core::{App, IntoRoute, Route};
use serverust_macros::{authorize, get as get_route, public};
use tower::ServiceExt;
use utoipa::openapi::HttpMethod;
use utoipa::openapi::path::Operation;

/// Rota protegida que **não pede `Auth<C>`**. É o caso que importa: sem o
/// default deny, esquecer o extractor publica o endpoint em silêncio.
#[get_route("/relatorio")]
async fn relatorio() -> &'static str {
    "dados-confidenciais"
}

/// Rota que pede identidade, para conferir que as duas formas convivem.
#[get_route("/eu")]
async fn eu(user: Auth<StandardClaims>) -> String {
    user.sub.clone()
}

/// Rota pública pela via programática.
struct Health;

impl IntoRoute for Health {
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

/// A mesma rota aberta, agora pela macro. Se `#[public]` e `Route::public()`
/// divergirem, um destes dois testes cai.
#[public]
#[get_route("/ping")]
async fn ping() -> &'static str {
    "pong"
}

/// Autorização por escopo, com o token real alimentando os fatos.
#[authorize(scope = "orders:read")]
#[get_route("/pedidos")]
async fn pedidos() -> &'static str {
    "pedidos"
}

fn app() -> axum::Router {
    App::new()
        .without_docs()
        .layer(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(
            common::SEGREDO,
        )))
        .route(relatorio)
        .route(eu)
        .route(Health)
        .route(ping)
        .route(pedidos)
        .into_router()
}

fn token_com_escopo(scope: &str) -> String {
    common::assinar_hs256(
        &json!({ "sub": "user-42", "exp": common::instante(3600), "scope": scope }),
        common::SEGREDO,
    )
}

/// O teste que o vão deixava passar: rota sem `Auth<C>` na assinatura,
/// protegida apenas pelo default deny.
#[tokio::test]
async fn rota_sem_extractor_e_negada_sem_token() {
    let resp = app()
        .oneshot(common::req_sem_token("/relatorio"))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "o AuthLayer real não ativou o default deny — rota sem Auth<C> ficou aberta"
    );
    // O motivo é o preciso do layer, não o genérico do portão: o `AuthFailure`
    // atravessa a rejeição, então o cliente não perde informação por a rota
    // ser protegida pelo default deny em vez do extractor.
    common::assert_401(resp, "missing_credentials").await;
}

#[tokio::test]
async fn rota_sem_extractor_responde_com_token_valido() {
    let resp = app()
        .oneshot(common::req_bearer("/relatorio", &common::token_valido()))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(common::corpo_texto(resp).await, "dados-confidenciais");
}

/// Token presente mas assinado por outro emissor não pode liberar o portão.
/// Cobre o ramo de erro do layer, onde `AuthEnabled` entra e `Authenticated`
/// não — se o marcador de identidade vazasse para esse ramo, este teste viraria
/// 200.
#[tokio::test]
async fn token_forjado_e_negado_em_rota_sem_extractor() {
    let forjado = common::assinar_hs256(&common::claims_validas(), common::OUTRO_SEGREDO);

    let resp = app()
        .oneshot(common::req_bearer("/relatorio", &forjado))
        .await
        .unwrap();

    common::assert_401(resp, "invalid_token").await;
}

#[tokio::test]
async fn token_expirado_e_negado_em_rota_sem_extractor() {
    let expirado = common::assinar_hs256(
        &json!({ "sub": "user-42", "exp": common::instante(-3600) }),
        common::SEGREDO,
    );

    let resp = app()
        .oneshot(common::req_bearer("/relatorio", &expirado))
        .await
        .unwrap();

    // `token_expired` diz ao cliente para renovar, não para reautenticar —
    // é a distinção que se perderia se o portão substituísse o motivo.
    common::assert_401(resp, "token_expired").await;
}

#[tokio::test]
async fn rota_publica_responde_sem_token_com_o_layer_real() {
    let resp = app()
        .oneshot(common::req_sem_token("/health"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(common::corpo_texto(resp).await, "ok");
}

#[tokio::test]
async fn rota_com_extractor_segue_entregando_a_identidade() {
    let resp = app()
        .oneshot(common::req_bearer("/eu", &common::token_valido()))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(common::corpo_texto(resp).await, "user-42");
}

/// `#[public]` com o `AuthLayer` real: a macro tem que chegar ao mesmo
/// resultado da via programática.
#[tokio::test]
async fn macro_public_abre_a_rota_com_o_layer_real() {
    let resp = app().oneshot(common::req_sem_token("/ping")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(common::corpo_texto(resp).await, "pong");
}

/// `#[authorize]` lendo os fatos que o `AuthLayer` publicou a partir de um
/// token de verdade — a ponte entre os dois crates, sem dublê.
#[tokio::test]
async fn macro_authorize_le_o_escopo_do_token_real() {
    let resp = app()
        .oneshot(common::req_bearer(
            "/pedidos",
            &token_com_escopo("orders:read orders:write"),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(common::corpo_texto(resp).await, "pedidos");
}

#[tokio::test]
async fn macro_authorize_nega_token_sem_o_escopo() {
    let resp = app()
        .oneshot(common::req_bearer("/pedidos", &token_com_escopo("profile")))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "token válido sem o escopo passou pelo #[authorize]"
    );
}

/// Sem token, quem rejeita é o portão — antes de o guard rodar — e o motivo
/// preciso do layer sobrevive.
#[tokio::test]
async fn macro_authorize_sem_token_e_401_do_portao() {
    let resp = app()
        .oneshot(common::req_sem_token("/pedidos"))
        .await
        .unwrap();

    common::assert_401(resp, "missing_credentials").await;
}

/// Sem o layer instalado, o portão é inerte — quem não usa autenticação não
/// muda de comportamento.
#[tokio::test]
async fn sem_o_layer_instalado_nada_e_negado() {
    let router = App::new().without_docs().route(relatorio).into_router();

    let resp = router
        .oneshot(common::req_sem_token("/relatorio"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

/// `App::auth()` precisa ativar o default deny tanto quanto `App::layer()`.
/// Um método nomeado "auth" que não autentica seria a repetição exata do
/// portão que existia sem fazer nada.
#[tokio::test]
async fn app_auth_ativa_o_default_deny() {
    let router = App::new()
        .without_docs()
        .auth(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(
            common::SEGREDO,
        )))
        .route(relatorio)
        .route(Health)
        .into_router();

    let negado = router
        .clone()
        .oneshot(common::req_sem_token("/relatorio"))
        .await
        .unwrap();
    assert_eq!(negado.status(), StatusCode::UNAUTHORIZED);

    let publica = router
        .oneshot(common::req_sem_token("/health"))
        .await
        .unwrap();
    assert_eq!(publica.status(), StatusCode::OK);
}

/// O inventário que o log de init imprime precisa bater com o que o portão
/// realmente deixa passar — senão o log vira uma declaração de segurança
/// falsa, que é pior do que não ter log.
#[tokio::test]
async fn o_inventario_bate_com_o_que_responde_sem_token() {
    let app = App::new()
        .without_docs()
        .auth(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(
            common::SEGREDO,
        )))
        .route(relatorio)
        .route(ping)
        .route(Health);

    let inventario: Vec<&str> = app.public_routes().iter().map(|(_, path)| *path).collect();
    assert_eq!(inventario, vec!["/ping", "/health"]);

    let router = app.into_router();
    for path in inventario {
        let resp = router
            .clone()
            .oneshot(common::req_sem_token(path))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{path} está no inventário de rotas públicas mas não responde sem token"
        );
    }
}

/// O escape hatch com o layer real: abre o que não foi anotado, sem tocar no
/// que já estava aberto.
#[tokio::test]
async fn allow_unannotated_com_o_layer_real() {
    let router = App::new()
        .without_docs()
        .auth(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(
            common::SEGREDO,
        )))
        .allow_unannotated()
        .route(relatorio)
        .into_router();

    let resp = router
        .oneshot(common::req_sem_token("/relatorio"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

/// Mas não desarma `#[authorize]`: afrouxar o default é uma coisa, ignorar a
/// permissão que alguém pediu de propósito é outra.
#[tokio::test]
async fn allow_unannotated_nao_desarma_authorize() {
    let router = App::new()
        .without_docs()
        .auth(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(
            common::SEGREDO,
        )))
        .allow_unannotated()
        .route(pedidos)
        .into_router();

    let resp = router
        .oneshot(common::req_bearer("/pedidos", &token_com_escopo("profile")))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        ".allow_unannotated() desarmou o #[authorize] — o escape hatch afrouxa \
         o default, não a autorização explícita"
    );
}
