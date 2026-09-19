//! Testes dos fatos de autorização expostos pelas claims.
//!
//! A regra que estes testes protegem é simples: autorização nunca é concedida
//! por omissão. Escopo ou papel ausente é `false`, e um tipo de claims que só
//! carrega identidade nega tudo.

mod common;

use std::sync::Arc;

use axum::routing::get;
use axum::{Extension, Router};
use common::{SEGREDO, assinar_hs256, corpo_json, instante, req_bearer};
use http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use serverust_auth::{AuthLayer, AuthzFacts, JwtAuth, StandardClaims};
use tower::ServiceExt;

fn claims_de(valor: serde_json::Value) -> StandardClaims {
    serde_json::from_value(valor).expect("StandardClaims desserializável")
}

// ---------------------------------------------------------------------------
// Escopos
// ---------------------------------------------------------------------------

#[test]
fn has_scope_le_da_string_separada_por_espaco() {
    // Formato da RFC 6749.
    let claims = claims_de(json!({
        "sub": "user-1",
        "scope": "read:funds write:funds openid",
    }));

    assert!(claims.has_scope("read:funds"));
    assert!(claims.has_scope("write:funds"));
    assert!(claims.has_scope("openid"));
}

#[test]
fn has_scope_le_do_array_scp() {
    // Formato usado por alguns emissores (Azure AD, entre outros).
    let claims = claims_de(json!({
        "sub": "user-1",
        "scp": ["read:funds", "openid"],
    }));

    assert!(claims.has_scope("read:funds"));
    assert!(claims.has_scope("openid"));
}

#[test]
fn has_scope_considera_scope_e_scp_ao_mesmo_tempo() {
    let claims = claims_de(json!({
        "sub": "user-1",
        "scope": "read:funds",
        "scp": ["write:funds"],
    }));

    assert!(claims.has_scope("read:funds"));
    assert!(claims.has_scope("write:funds"));
}

#[test]
fn escopo_ausente_e_false() {
    let claims = claims_de(json!({
        "sub": "user-1",
        "scope": "read:funds",
        "scp": ["openid"],
    }));

    assert!(!claims.has_scope("delete:funds"));
    assert!(!claims.has_scope(""));
}

#[test]
fn has_scope_nao_casa_prefixo_nem_substring() {
    // "read" não é "read:funds": casar por substring concederia escopo a mais.
    let claims = claims_de(json!({ "sub": "user-1", "scope": "read:funds" }));

    assert!(!claims.has_scope("read"));
    assert!(!claims.has_scope("funds"));
    assert!(!claims.has_scope("read:fund"));
}

#[test]
fn claims_sem_nenhum_escopo_negam_tudo() {
    let claims = claims_de(json!({ "sub": "user-1" }));

    assert!(!claims.has_scope("read:funds"));
    assert!(!claims.has_role("admin"));
}

// ---------------------------------------------------------------------------
// Papéis
// ---------------------------------------------------------------------------

#[test]
fn has_role_le_de_roles() {
    let claims = claims_de(json!({ "sub": "user-1", "roles": ["admin", "auditor"] }));

    assert!(claims.has_role("admin"));
    assert!(claims.has_role("auditor"));
}

#[test]
fn has_role_le_de_groups() {
    // Grupos são tratados como papéis.
    let claims = claims_de(json!({ "sub": "user-1", "groups": ["financeiro"] }));

    assert!(claims.has_role("financeiro"));
}

#[test]
fn papel_ausente_e_false() {
    let claims = claims_de(json!({
        "sub": "user-1",
        "roles": ["auditor"],
        "groups": ["financeiro"],
    }));

    assert!(!claims.has_role("admin"));
}

#[test]
fn subject_espelha_a_claim_sub() {
    let claims = claims_de(json!({ "sub": "user-1" }));

    assert_eq!(claims.subject(), "user-1");
}

// ---------------------------------------------------------------------------
// Defaults da trait: um tipo só de identidade nega toda autorização
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ClaimsMinimas {
    sub: String,
}

impl AuthzFacts for ClaimsMinimas {
    fn subject(&self) -> &str {
        &self.sub
    }
}

#[test]
fn tipo_sem_autorizacao_usa_defaults_negativos() {
    let claims = ClaimsMinimas {
        sub: "user-1".into(),
    };

    assert_eq!(claims.subject(), "user-1");
    assert!(!claims.has_scope("read:funds"));
    assert!(!claims.has_role("admin"));
}

// ---------------------------------------------------------------------------
// Fatos apagados de tipo depositados pelo layer
// ---------------------------------------------------------------------------

/// Consome `Arc<dyn AuthzFacts>` — é assim que um guard genérico, que não
/// conhece o tipo concreto de claims, lê a autorização.
async fn fatos(Extension(facts): Extension<Arc<dyn AuthzFacts>>) -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "subject": facts.subject(),
        "tem_leitura": facts.has_scope("read:funds"),
        "tem_admin": facts.has_role("admin"),
    }))
}

#[tokio::test]
async fn layer_deposita_fatos_apagados_de_tipo() {
    let router = Router::new()
        .route("/fatos", get(fatos))
        .layer(AuthLayer::<StandardClaims>::new(JwtAuth::hs256(SEGREDO)));

    let token = assinar_hs256(
        &json!({
            "sub": "user-42",
            "exp": instante(3600),
            "scp": ["read:funds"],
            "groups": ["admin"],
        }),
        SEGREDO,
    );

    let resp = router
        .oneshot(req_bearer("/fatos", &token))
        .await
        .expect("o router não pode falhar");

    assert_eq!(resp.status(), StatusCode::OK);
    let corpo = corpo_json(resp).await;
    assert_eq!(corpo["subject"], "user-42");
    assert_eq!(corpo["tem_leitura"], true);
    assert_eq!(corpo["tem_admin"], true);
}

#[tokio::test]
async fn layer_com_claims_minimas_nega_escopo_e_papel() {
    let router = Router::new()
        .route("/fatos", get(fatos))
        .layer(AuthLayer::<ClaimsMinimas>::new(JwtAuth::hs256(SEGREDO)));

    // O token até traz escopos, mas `ClaimsMinimas` não os lê: os defaults
    // da trait valem e nada é concedido.
    let token = assinar_hs256(
        &json!({
            "sub": "user-42",
            "exp": instante(3600),
            "scope": "read:funds",
            "roles": ["admin"],
        }),
        SEGREDO,
    );

    let resp = router
        .oneshot(req_bearer("/fatos", &token))
        .await
        .expect("o router não pode falhar");

    assert_eq!(resp.status(), StatusCode::OK);
    let corpo = corpo_json(resp).await;
    assert_eq!(corpo["subject"], "user-42");
    assert_eq!(corpo["tem_leitura"], false);
    assert_eq!(corpo["tem_admin"], false);
}
