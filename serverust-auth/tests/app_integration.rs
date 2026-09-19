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

/// Rota genuinamente pública, pela via programática — a macro `#[public]`
/// ainda não existe, então `Route::public()` é o contrato disponível.
struct AnonimaPublica;

impl serverust_core::IntoRoute for AnonimaPublica {
    fn into_route(self) -> serverust_core::Route {
        serverust_core::Route::new(
            "/anonima",
            utoipa::openapi::HttpMethod::Get,
            axum::routing::get(|user: MaybeAuth<StandardClaims>| async move {
                axum::Json(serde_json::json!({ "autenticado": user.0.is_some() }))
            }),
            utoipa::openapi::path::Operation::new(),
        )
        .public()
    }
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

/// `MaybeAuth` na assinatura **não** torna a rota pública.
///
/// Antes do default deny este teste afirmava o contrário, porque o extractor
/// opcional nunca rejeita e a rota respondia. Com o portão instalado, quem
/// decide é a marcação da rota — e uma rota apenas *chamada* de pública
/// continua sendo negada. Nomear não é anotar.
#[tokio::test]
async fn maybe_auth_nao_torna_a_rota_publica() {
    let resp = app()
        .oneshot(req_sem_token("/publica"))
        .await
        .expect("o router não pode falhar");

    assert_401(resp, "missing_credentials").await;
}

/// Em rota genuinamente pública, o `MaybeAuth` volta a fazer o seu papel:
/// responde sem token, informando que não há identidade.
#[tokio::test]
async fn maybe_auth_devolve_none_em_rota_marcada_publica() {
    let router = App::new()
        .without_docs()
        .layer(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(SEGREDO)))
        .route(AnonimaPublica)
        .into_router();

    let resp = router
        .oneshot(req_sem_token("/anonima"))
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
