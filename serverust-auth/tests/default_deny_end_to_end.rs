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
use serverust_macros::get as get_route;
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

fn app() -> axum::Router {
    App::new()
        .without_docs()
        .layer(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(
            common::SEGREDO,
        )))
        .route(relatorio)
        .route(eu)
        .route(Health)
        .into_router()
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
