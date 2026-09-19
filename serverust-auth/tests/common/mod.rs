//! Utilidades compartilhadas pelos testes de integração do `serverust-auth`.
//!
//! Emitir token é papel do teste, não do crate: o crate só verifica. Por isso
//! a assinatura acontece aqui, com `jsonwebtoken` declarado em
//! `[dev-dependencies]` com as mesmas features das `[dependencies]`.

// Cada arquivo em `tests/` vira um binário próprio e usa apenas parte destes
// helpers; sem este allow o compilador acusa dead_code nos que sobram.
#![allow(dead_code)]

use axum::body::Body;
use axum::response::Response;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, get_current_timestamp};
use serde_json::{Value, json};

/// Segredo usado por quem emite E por quem verifica no caminho feliz.
pub const SEGREDO: &[u8] = b"segredo-de-teste-hs256-bem-longo";

/// Segredo de um emissor que não é o nosso — serve para forjar assinatura.
pub const OUTRO_SEGREDO: &[u8] = b"segredo-de-um-impostor-qualquer";

/// Emissor esperado nos testes que configuram `.issuer(...)`.
pub const EMISSOR: &str = "https://idp.exemplo.com/";

/// Audiência esperada nos testes que configuram `.audience(...)`.
pub const AUDIENCIA: &str = "minha-api";

/// Instante, em segundos desde a época Unix, deslocado do agora.
pub fn instante(offset_segundos: i64) -> u64 {
    let agora = get_current_timestamp() as i64;
    (agora + offset_segundos) as u64
}

/// Claims mínimas e válidas: `sub` e `exp` no futuro.
pub fn claims_validas() -> Value {
    json!({
        "sub": "user-42",
        "exp": instante(3600),
    })
}

/// Assina claims arbitrárias com HS256.
pub fn assinar_hs256(claims: &Value, segredo: &[u8]) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(segredo),
    )
    .expect("assinatura HS256 de teste não pode falhar")
}

/// Token HS256 válido, assinado com [`SEGREDO`].
pub fn token_valido() -> String {
    assinar_hs256(&claims_validas(), SEGREDO)
}

/// `GET` sem header `Authorization`.
pub fn req_sem_token(path: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .expect("request de teste bem formada")
}

/// `GET` com o valor bruto do header `Authorization` — bruto de propósito,
/// para que os testes possam enviar lixo deliberado.
pub fn req_com_authorization(path: &str, valor: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(http::header::AUTHORIZATION, valor)
        .body(Body::empty())
        .expect("request de teste bem formada")
}

/// `GET` com `Authorization: Bearer <token>`.
pub fn req_bearer(path: &str, token: &str) -> Request<Body> {
    req_com_authorization(path, &format!("Bearer {token}"))
}

/// Corpo da resposta desserializado como JSON.
pub async fn corpo_json(resp: Response) -> Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("corpo legível")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("corpo precisa ser JSON")
}

/// Afirma o contrato completo de uma resposta 401: status, header
/// `WWW-Authenticate` e corpo `{"error":"unauthorized","reason":"..."}`.
pub async fn assert_401(resp: Response, razao_esperada: &str) {
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "esperava 401 com reason={razao_esperada}"
    );
    assert_eq!(
        resp.headers()
            .get(http::header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok()),
        Some("Bearer"),
        "401 sem WWW-Authenticate não diz ao cliente qual esquema usar (RFC 6750 §3)"
    );

    let corpo = corpo_json(resp).await;
    assert_eq!(corpo["error"], "unauthorized");
    assert_eq!(
        corpo["reason"], razao_esperada,
        "o código de reason faz parte da API pública: clientes ramificam sobre ele"
    );
}
