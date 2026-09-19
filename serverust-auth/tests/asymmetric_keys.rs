//! Testes das chaves assimétricas: RS256 e ES256.
//!
//! Os pares em `tests/fixtures/` são fixos e existem só para teste — nunca
//! foram nem serão usados para assinar nada de verdade. Ter par fixo mantém o
//! teste determinístico e rápido: gerar RSA de 2048 bits a cada execução
//! custaria mais que todo o resto da suíte.

mod common;

use axum::Router;
use axum::routing::get;
use common::{assert_401, corpo_json, instante, req_bearer};
use http::StatusCode;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::json;
use serverust_auth::{Auth, AuthError, AuthLayer, JwtAuth, StandardClaims};
use tower::ServiceExt;

const RSA_PRIVADA: &[u8] = include_bytes!("fixtures/rs256_private.pem");
const RSA_PUBLICA: &[u8] = include_bytes!("fixtures/rs256_public.pem");
const EC_PRIVADA: &[u8] = include_bytes!("fixtures/es256_private.pem");
const EC_PUBLICA: &[u8] = include_bytes!("fixtures/es256_public.pem");

async fn me(user: Auth<StandardClaims>) -> axum::Json<serde_json::Value> {
    axum::Json(json!({ "sub": user.sub }))
}

fn router(auth: JwtAuth) -> Router {
    Router::new()
        .route("/me", get(me))
        .layer(AuthLayer::<StandardClaims>::new(auth))
}

fn assinar(alg: Algorithm, chave: &EncodingKey) -> String {
    let claims = json!({ "sub": "user-42", "exp": instante(3600) });
    encode(&Header::new(alg), &claims, chave).expect("assinatura de teste não pode falhar")
}

// ---------------------------------------------------------------------------
// RS256
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rs256_aceita_token_assinado_pela_chave_privada_do_par() {
    let chave = EncodingKey::from_rsa_pem(RSA_PRIVADA).expect("PEM RSA privado válido");
    let token = assinar(Algorithm::RS256, &chave);
    let auth = JwtAuth::rs256_pem(RSA_PUBLICA).expect("PEM RSA público válido");

    let resp = router(auth)
        .oneshot(req_bearer("/me", &token))
        .await
        .expect("o router não pode falhar");

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(corpo_json(resp).await["sub"], "user-42");
}

#[tokio::test]
async fn rs256_recusa_token_de_outra_chave() {
    // Assinado com HS256 usando o PEM público como "segredo": é exatamente a
    // confusão de algoritmo que o `alg` fixo no construtor fecha.
    let token = assinar(Algorithm::HS256, &EncodingKey::from_secret(RSA_PUBLICA));
    let auth = JwtAuth::rs256_pem(RSA_PUBLICA).expect("PEM RSA público válido");

    let resp = router(auth)
        .oneshot(req_bearer("/me", &token))
        .await
        .expect("o router não pode falhar");

    assert_401(resp, "invalid_token").await;
}

#[test]
fn rs256_com_pem_invalido_devolve_invalid_key() {
    let err = JwtAuth::rs256_pem(b"isto nao e um PEM").expect_err("PEM lixo precisa falhar");

    assert!(matches!(err, AuthError::InvalidKey(_)));
    assert_eq!(err.reason(), "invalid_key");
}

// ---------------------------------------------------------------------------
// ES256
// ---------------------------------------------------------------------------

#[tokio::test]
async fn es256_aceita_token_assinado_pela_chave_privada_do_par() {
    let chave = EncodingKey::from_ec_pem(EC_PRIVADA).expect("PEM EC privado válido");
    let token = assinar(Algorithm::ES256, &chave);
    let auth = JwtAuth::es256_pem(EC_PUBLICA).expect("PEM EC público válido");

    let resp = router(auth)
        .oneshot(req_bearer("/me", &token))
        .await
        .expect("o router não pode falhar");

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(corpo_json(resp).await["sub"], "user-42");
}

#[tokio::test]
async fn es256_recusa_token_rs256_mesmo_com_par_valido() {
    // Algoritmo divergente do configurado é token inválido, não 500.
    let chave = EncodingKey::from_rsa_pem(RSA_PRIVADA).expect("PEM RSA privado válido");
    let token = assinar(Algorithm::RS256, &chave);
    let auth = JwtAuth::es256_pem(EC_PUBLICA).expect("PEM EC público válido");

    let resp = router(auth)
        .oneshot(req_bearer("/me", &token))
        .await
        .expect("o router não pode falhar");

    assert_401(resp, "invalid_token").await;
}

#[test]
fn es256_com_pem_invalido_devolve_invalid_key() {
    let err = JwtAuth::es256_pem(b"-----BEGIN PUBLIC KEY-----\nlixo\n-----END PUBLIC KEY-----")
        .expect_err("PEM lixo precisa falhar");

    assert!(matches!(err, AuthError::InvalidKey(_)));
    assert_eq!(err.reason(), "invalid_key");
}
