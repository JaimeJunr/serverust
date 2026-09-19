//! Testes do caminho HTTP completo com verificação de JWT por chave estática.
//!
//! Tudo aqui passa pelo pipeline real: `AuthLayer` valida a requisição e os
//! extractors `Auth` / `MaybeAuth` decidem o que fazer com o resultado. O que
//! se afirma é o contrato observável pelo cliente — status, header e corpo —
//! e não o estado interno do crate.

mod common;

use axum::body::Body;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use common::{
    AUDIENCIA, EMISSOR, OUTRO_SEGREDO, SEGREDO, assert_401, assinar_hs256, claims_validas,
    corpo_json, instante, req_bearer, req_com_authorization, req_sem_token, token_valido,
};
use http::{Request, StatusCode};
use serde_json::{Value, json};
use serverust_auth::{Auth, AuthLayer, AuthzFacts, JwtAuth, MaybeAuth, StandardClaims};
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Handlers e routers de apoio
// ---------------------------------------------------------------------------

/// Rota protegida: exige identidade e devolve o que leu das claims.
async fn me(user: Auth<StandardClaims>) -> Json<Value> {
    Json(json!({
        "sub": user.sub,
        "iss": user.iss,
        "subject": user.subject(),
        "tem_leitura": user.has_scope("read:funds"),
        "tem_admin": user.has_role("admin"),
    }))
}

/// Rota pública que muda de comportamento quando há identidade.
async fn quem_sou(user: MaybeAuth<StandardClaims>) -> Json<Value> {
    match user.0 {
        Some(claims) => Json(json!({ "autenticado": true, "sub": claims.sub })),
        None => Json(json!({ "autenticado": false })),
    }
}

/// Router com o layer instalado sobre as duas rotas.
fn router_com_layer(auth: JwtAuth) -> Router {
    Router::new()
        .route("/me", get(me))
        .route("/quem-sou", get(quem_sou))
        .layer(AuthLayer::<StandardClaims>::new(auth))
}

/// Router HS256 com o segredo padrão dos testes.
fn router_hs256() -> Router {
    router_com_layer(JwtAuth::hs256(SEGREDO))
}

async fn chamar(router: Router, req: Request<Body>) -> Response {
    router.oneshot(req).await.expect("o router não pode falhar")
}

// ---------------------------------------------------------------------------
// Caminho feliz
// ---------------------------------------------------------------------------

#[tokio::test]
async fn token_valido_devolve_200_com_as_claims_no_handler() {
    let claims = json!({
        "sub": "user-42",
        "iss": EMISSOR,
        "exp": instante(3600),
        "scope": "read:funds write:funds",
        "roles": ["admin"],
    });
    let token = assinar_hs256(&claims, SEGREDO);

    let resp = chamar(router_hs256(), req_bearer("/me", &token)).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let corpo = corpo_json(resp).await;
    assert_eq!(corpo["sub"], "user-42");
    assert_eq!(corpo["iss"], EMISSOR);
    assert_eq!(
        corpo["subject"], "user-42",
        "subject() precisa espelhar a claim sub"
    );
    assert_eq!(corpo["tem_leitura"], true);
    assert_eq!(corpo["tem_admin"], true);
}

#[tokio::test]
async fn issuer_e_audiencia_corretos_passam() {
    let claims = json!({
        "sub": "user-42",
        "iss": EMISSOR,
        "aud": AUDIENCIA,
        "exp": instante(3600),
    });
    let token = assinar_hs256(&claims, SEGREDO);
    let auth = JwtAuth::hs256(SEGREDO).issuer(EMISSOR).audience(AUDIENCIA);

    let resp = chamar(router_com_layer(auth), req_bearer("/me", &token)).await;

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn leeway_amplia_a_tolerancia_padrao_de_relogio() {
    // O default do `jsonwebtoken` já é 60s de tolerância; `.leeway(600)` a
    // amplia. Um token expirado há 5 min cai fora do default e dentro do
    // configurado — é a diferença que este teste isola.
    let claims = json!({ "sub": "user-42", "exp": instante(-300) });
    let token = assinar_hs256(&claims, SEGREDO);

    let padrao = chamar(router_hs256(), req_bearer("/me", &token)).await;
    assert_401(padrao, "token_expired").await;

    let auth = JwtAuth::hs256(SEGREDO).leeway(600);
    let ampliado = chamar(router_com_layer(auth), req_bearer("/me", &token)).await;
    assert_eq!(ampliado.status(), StatusCode::OK);
}

#[tokio::test]
async fn tolerancia_padrao_aceita_token_expirado_ha_poucos_segundos() {
    // Documenta o default herdado do `jsonwebtoken`: 60s de folga para
    // desalinhamento de relógio, mesmo sem `.leeway(...)` explícito. Se este
    // teste mudar de cor, o default de tolerância mudou junto.
    let claims = json!({ "sub": "user-42", "exp": instante(-5) });
    let token = assinar_hs256(&claims, SEGREDO);

    let resp = chamar(router_hs256(), req_bearer("/me", &token)).await;

    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Credencial ausente ou malformada
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sem_header_authorization_devolve_401_missing_credentials() {
    let resp = chamar(router_hs256(), req_sem_token("/me")).await;

    assert_401(resp, "missing_credentials").await;
}

#[tokio::test]
async fn header_sem_esquema_devolve_401_malformed() {
    // Token cru, sem o "Bearer " na frente.
    let resp = chamar(
        router_hs256(),
        req_com_authorization("/me", &token_valido()),
    )
    .await;

    assert_401(resp, "malformed_authorization_header").await;
}

#[tokio::test]
async fn esquema_errado_devolve_401_malformed() {
    let resp = chamar(
        router_hs256(),
        req_com_authorization("/me", "Basic dXNlcjpzZW5oYQ=="),
    )
    .await;

    assert_401(resp, "malformed_authorization_header").await;
}

#[tokio::test]
async fn bearer_com_token_vazio_devolve_401_malformed() {
    let resp = chamar(router_hs256(), req_com_authorization("/me", "Bearer ")).await;

    assert_401(resp, "malformed_authorization_header").await;
}

#[tokio::test]
async fn bearer_sozinho_devolve_401_malformed() {
    let resp = chamar(router_hs256(), req_com_authorization("/me", "Bearer")).await;

    assert_401(resp, "malformed_authorization_header").await;
}

#[tokio::test]
async fn esquema_bearer_e_case_insensitive() {
    // RFC 7235 §2.1: o nome do esquema é comparado sem diferenciar caixa.
    for esquema in ["bearer", "BEARER", "BeArEr"] {
        let valor = format!("{esquema} {}", token_valido());
        let resp = chamar(router_hs256(), req_com_authorization("/me", &valor)).await;

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "esquema {esquema:?} precisa ser aceito"
        );
    }
}

// ---------------------------------------------------------------------------
// Token inválido
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assinatura_de_outro_segredo_devolve_401_invalid_token() {
    let token = assinar_hs256(&claims_validas(), OUTRO_SEGREDO);

    let resp = chamar(router_hs256(), req_bearer("/me", &token)).await;

    assert_401(resp, "invalid_token").await;
}

#[tokio::test]
async fn token_que_nao_e_jwt_devolve_401_invalid_token() {
    let resp = chamar(router_hs256(), req_bearer("/me", "isto-nao-e-um-jwt")).await;

    assert_401(resp, "invalid_token").await;
}

#[tokio::test]
async fn token_sem_claim_sub_devolve_401_invalid_token() {
    // `sub` é obrigatório em `StandardClaims`: payload indesserializável é
    // token inválido, não erro 500.
    let token = assinar_hs256(&json!({ "exp": instante(3600) }), SEGREDO);

    let resp = chamar(router_hs256(), req_bearer("/me", &token)).await;

    assert_401(resp, "invalid_token").await;
}

#[tokio::test]
async fn token_expirado_devolve_401_token_expired() {
    let claims = json!({ "sub": "user-42", "exp": instante(-3600) });
    let token = assinar_hs256(&claims, SEGREDO);

    let resp = chamar(router_hs256(), req_bearer("/me", &token)).await;

    assert_401(resp, "token_expired").await;
}

#[tokio::test]
async fn token_sem_exp_devolve_401_invalid_token() {
    // `Validation::new` exige `exp`: um token sem expiração nunca vale.
    let token = assinar_hs256(&json!({ "sub": "user-42" }), SEGREDO);

    let resp = chamar(router_hs256(), req_bearer("/me", &token)).await;

    assert_401(resp, "invalid_token").await;
}

#[tokio::test]
async fn issuer_errado_devolve_401_invalid_issuer() {
    let claims = json!({
        "sub": "user-42",
        "iss": "https://idp-do-atacante.exemplo/",
        "exp": instante(3600),
    });
    let token = assinar_hs256(&claims, SEGREDO);
    let auth = JwtAuth::hs256(SEGREDO).issuer(EMISSOR);

    let resp = chamar(router_com_layer(auth), req_bearer("/me", &token)).await;

    assert_401(resp, "invalid_issuer").await;
}

#[tokio::test]
async fn audiencia_errada_devolve_401_invalid_audience() {
    let claims = json!({
        "sub": "user-42",
        "aud": "outra-api",
        "exp": instante(3600),
    });
    let token = assinar_hs256(&claims, SEGREDO);
    let auth = JwtAuth::hs256(SEGREDO).audience(AUDIENCIA);

    let resp = chamar(router_com_layer(auth), req_bearer("/me", &token)).await;

    assert_401(resp, "invalid_audience").await;
}

// ---------------------------------------------------------------------------
// Falha fechada
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_sem_layer_instalado_nega_a_requisicao() {
    // Garantia central do crate: esquecer o `AuthLayer` não abre a rota. Sem
    // o layer não há claims nas extensions, e `Auth` nega em vez de deixar
    // passar. Este teste documenta essa garantia — se ele ficar vermelho,
    // rotas protegidas viraram públicas.
    let router = Router::new().route("/me", get(me));

    let resp = chamar(router, req_bearer("/me", &token_valido())).await;

    assert_401(resp, "missing_credentials").await;
}

#[tokio::test]
async fn resposta_401_sempre_traz_www_authenticate_bearer() {
    let resp = chamar(router_hs256(), req_sem_token("/me")).await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        resp.headers()
            .get(http::header::WWW_AUTHENTICATE)
            .expect("401 precisa de WWW-Authenticate"),
        "Bearer"
    );
}

// ---------------------------------------------------------------------------
// MaybeAuth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn maybe_auth_devolve_none_sem_token() {
    let resp = chamar(router_hs256(), req_sem_token("/quem-sou")).await;

    assert_eq!(resp.status(), StatusCode::OK, "MaybeAuth nunca rejeita");
    assert_eq!(corpo_json(resp).await["autenticado"], false);
}

#[tokio::test]
async fn maybe_auth_devolve_some_com_token_valido() {
    let resp = chamar(router_hs256(), req_bearer("/quem-sou", &token_valido())).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let corpo = corpo_json(resp).await;
    assert_eq!(corpo["autenticado"], true);
    assert_eq!(corpo["sub"], "user-42");
}

#[tokio::test]
async fn maybe_auth_devolve_none_com_token_invalido() {
    // Token ruim não é identidade: a rota responde, mas como anônima.
    let token = assinar_hs256(&claims_validas(), OUTRO_SEGREDO);

    let resp = chamar(router_hs256(), req_bearer("/quem-sou", &token)).await;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(corpo_json(resp).await["autenticado"], false);
}
