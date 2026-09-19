//! Integração com o framework: `App::layer` + rotas declaradas por macro.
//!
//! Prova que o `AuthLayer` satisfaz os bounds de `App::layer` e que uma rota
//! anotada com `#[get]` consegue pedir `Auth` na assinatura — o caminho que a
//! documentação do crate ensina.

mod common;

use common::{SEGREDO, assert_401, corpo_json, instante, req_bearer, req_sem_token};
use http::StatusCode;
use serverust_auth::{Auth, AuthLayer, JwtAuth, MaybeAuth, StandardClaims};
use serverust_core::App;
use serverust_macros::get;
use tower::ServiceExt;

#[get("/me")]
async fn me(user: Auth<StandardClaims>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "sub": user.sub }))
}

#[get("/publica")]
async fn publica(user: MaybeAuth<StandardClaims>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "autenticado": user.0.is_some() }))
}

fn app() -> axum::Router {
    App::new()
        .layer(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(SEGREDO)))
        .route(me)
        .route(publica)
        .into_router()
}

fn token() -> String {
    common::assinar_hs256(
        &serde_json::json!({ "sub": "user-42", "exp": instante(3600) }),
        SEGREDO,
    )
}

#[tokio::test]
async fn rota_de_macro_recebe_identidade_pelo_layer_do_app() {
    let resp = app()
        .oneshot(req_bearer("/me", &token()))
        .await
        .expect("o router não pode falhar");

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(corpo_json(resp).await["sub"], "user-42");
}

#[tokio::test]
async fn rota_de_macro_sem_token_devolve_401() {
    let resp = app()
        .oneshot(req_sem_token("/me"))
        .await
        .expect("o router não pode falhar");

    assert_401(resp, "missing_credentials").await;
}

#[tokio::test]
async fn rota_publica_responde_sem_token_com_o_layer_instalado() {
    // O layer roda em todas as rotas e não rejeita nenhuma: quem rejeita é o
    // extractor. Uma rota pública sob o mesmo layer segue respondendo.
    let resp = app()
        .oneshot(req_sem_token("/publica"))
        .await
        .expect("o router não pode falhar");

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(corpo_json(resp).await["autenticado"], false);
}

#[tokio::test]
async fn rotas_de_documentacao_seguem_acessiveis_sem_token() {
    let resp = app()
        .oneshot(req_sem_token("/openapi.json"))
        .await
        .expect("o router não pode falhar");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "/openapi.json fica fora da pipeline de middleware do usuário"
    );
}
