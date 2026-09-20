//! Descoberta de OIDC e verificação com chaves do JWKS (ADR 0009, etapa 4).
//!
//! Os testes sobem um **IdP falso de verdade**: um axum local servindo o
//! documento de descoberta e o JWKS por HTTP. Não há dublê do cliente HTTP em
//! lugar nenhum — o caminho exercitado é o mesmo que roda em produção, menos o
//! TLS.
//!
//! Isso é deliberado. A lição do portão que existia sem fazer nada é que
//! contrato coberto só por dublê dos dois lados não prova que as pontas se
//! encaixam.

#![cfg(feature = "jwks")]

mod common;

use std::net::SocketAddr;

use axum::Router;
use axum::routing::get;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, get_current_timestamp};
use serde_json::{Value, json};
use serverust_auth::{AuthError, JwksAuth, StandardClaims};

const KID: &str = "chave-de-teste-1";
const JWKS: &str = include_str!("fixtures/rs256_jwks.json");
const CHAVE_PRIVADA: &[u8] = include_bytes!("fixtures/rs256_private.pem");

// ---------------------------------------------------------------------------
// IdP falso
// ---------------------------------------------------------------------------

/// Sobe um IdP local e devolve o `issuer` (a base URL) dele.
///
/// `documento` recebe o issuer efetivo e devolve o corpo de
/// `/.well-known/openid-configuration` — assim cada teste pode servir um
/// documento torto sem precisar de outro servidor.
async fn subir_idp(
    documento: impl Fn(String) -> Value + Clone + Send + Sync + 'static,
    jwks: &'static str,
) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("porta efêmera");
    let addr: SocketAddr = listener.local_addr().expect("endereço local");
    let issuer = format!("http://127.0.0.1:{}", addr.port());

    let issuer_do_doc = issuer.clone();
    let app = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(move || {
                let corpo = documento(issuer_do_doc.clone());
                async move { axum::Json(corpo) }
            }),
        )
        .route(
            "/jwks",
            get(move || async move { ([(http::header::CONTENT_TYPE, "application/json")], jwks) }),
        );

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("servidor do IdP");
    });

    issuer
}

/// Documento de descoberta bem-formado.
fn documento_padrao(issuer: String) -> Value {
    json!({
        "issuer": issuer,
        "jwks_uri": format!("{issuer}/jwks"),
    })
}

fn token_rs256(kid: Option<&str>, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = kid.map(str::to_string);
    encode(
        &header,
        claims,
        &EncodingKey::from_rsa_pem(CHAVE_PRIVADA).expect("chave privada de teste"),
    )
    .expect("assinatura")
}

fn claims(issuer: &str) -> Value {
    json!({
        "sub": "user-42",
        "iss": issuer,
        "exp": get_current_timestamp() + 3600,
    })
}

// ---------------------------------------------------------------------------
// Caminho feliz
// ---------------------------------------------------------------------------

#[tokio::test]
async fn descoberta_carrega_as_chaves_e_verifica_token() {
    let issuer = subir_idp(documento_padrao, JWKS).await;

    let auth = JwksAuth::discover(&issuer).await.expect("descoberta");
    assert_eq!(auth.key_count(), 1);
    assert!(auth.has_key(KID));

    let token = token_rs256(Some(KID), &claims(&issuer));
    let verificadas: StandardClaims = auth.verify(&token).expect("token válido");

    assert_eq!(verificadas.sub, "user-42");
}

/// A descoberta aplica o `iss` do documento como emissor esperado, sem o
/// usuário pedir. Um token de outro emissor precisa ser recusado por isso.
#[tokio::test]
async fn descoberta_liga_a_validacao_de_issuer_por_default() {
    let issuer = subir_idp(documento_padrao, JWKS).await;
    let auth = JwksAuth::discover(&issuer).await.expect("descoberta");

    let token = token_rs256(Some(KID), &claims("https://outro-emissor.exemplo/"));

    assert_eq!(
        auth.verify::<StandardClaims>(&token).unwrap_err(),
        AuthError::InvalidIssuer,
        "a descoberta não aplicou o issuer — `iss` passou sem ser verificada"
    );
}

#[tokio::test]
async fn from_jwks_uri_carrega_sem_passar_pela_descoberta() {
    let issuer = subir_idp(documento_padrao, JWKS).await;

    let auth = JwksAuth::from_jwks_uri(&format!("{issuer}/jwks"))
        .await
        .expect("jwks direto");

    assert!(auth.has_key(KID));
}

// ---------------------------------------------------------------------------
// Escolha da chave
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kid_desconhecido_tem_codigo_proprio() {
    let issuer = subir_idp(documento_padrao, JWKS).await;
    let auth = JwksAuth::discover(&issuer).await.expect("descoberta");

    let token = token_rs256(Some("kid-que-nao-existe"), &claims(&issuer));

    assert_eq!(
        auth.verify::<StandardClaims>(&token).unwrap_err(),
        AuthError::UnknownKeyId,
        "kid desconhecido precisa ser distinguível de token inválido: quem \
         precisa agir é o operador, não o cliente"
    );
}

/// Com uma única chave no JWKS, token sem `kid` é aceitável — não há escolha a
/// fazer.
#[tokio::test]
async fn token_sem_kid_usa_a_unica_chave() {
    let issuer = subir_idp(documento_padrao, JWKS).await;
    let auth = JwksAuth::discover(&issuer).await.expect("descoberta");

    let token = token_rs256(None, &claims(&issuer));

    assert!(auth.verify::<StandardClaims>(&token).is_ok());
}

// ---------------------------------------------------------------------------
// Confusão de algoritmo
// ---------------------------------------------------------------------------

/// O algoritmo sai da chave, nunca do header do token.
///
/// Este é o ataque clássico contra verificadores de JWKS: o atacante declara
/// `alg: HS256` no header e assina com a chave **pública** RSA do emissor, que
/// é conhecida. Se o verificador lesse o `alg` do token, usaria o módulo RSA
/// como segredo HMAC e a assinatura fecharia.
#[tokio::test]
async fn alg_do_header_do_token_nao_escolhe_o_algoritmo() {
    let issuer = subir_idp(documento_padrao, JWKS).await;
    let auth = JwksAuth::discover(&issuer).await.expect("descoberta");

    // A chave pública, tal como qualquer um a obtém do JWKS.
    let publica = include_bytes!("fixtures/rs256_public.pem");

    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(KID.to_string());
    let forjado = encode(
        &header,
        &claims(&issuer),
        &EncodingKey::from_secret(publica),
    )
    .expect("token forjado");

    assert_eq!(
        auth.verify::<StandardClaims>(&forjado).unwrap_err(),
        AuthError::InvalidToken,
        "o verificador aceitou o algoritmo declarado pelo token — confusão de \
         algoritmo está aberta"
    );
}

/// Chave simétrica publicada num JWKS é o próprio segredo de assinatura.
/// Aceitá-la significaria que qualquer leitor do JWKS forja tokens.
#[test]
fn chave_simetrica_no_jwks_e_recusada() {
    let jwks = json!({
        "keys": [{
            "kty": "oct",
            "alg": "HS256",
            "kid": "simetrica",
            "k": "c2VncmVkby1jb21wYXJ0aWxoYWRv",
        }]
    })
    .to_string();

    let err = JwksAuth::from_jwks_json(&jwks).unwrap_err();

    assert!(
        matches!(err, AuthError::InvalidKey(_)),
        "JWKS só com chave simétrica precisa falhar na construção, não virar \
         verificador que valida com um segredo público — erro: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Confiança na descoberta
// ---------------------------------------------------------------------------

/// A RFC 8414 §3.3 exige que o `issuer` do documento seja o mesmo usado para
/// chegar nele. Divergir e continuar faria a validação de `iss` atestar um
/// emissor que não é o configurado.
#[tokio::test]
async fn documento_com_issuer_divergente_e_recusado() {
    let issuer = subir_idp(
        |issuer| {
            json!({
                "issuer": "https://emissor-que-eu-nao-pedi.exemplo/",
                "jwks_uri": format!("{issuer}/jwks"),
            })
        },
        JWKS,
    )
    .await;

    let err = JwksAuth::discover(&issuer).await.unwrap_err();

    assert!(
        matches!(err, AuthError::Discovery(_)),
        "descoberta aceitou documento que descreve outro emissor: {err:?}"
    );
}

#[tokio::test]
async fn http_sem_tls_fora_de_loopback_e_recusado() {
    let err = JwksAuth::discover("http://idp.exemplo.com/")
        .await
        .unwrap_err();

    match err {
        AuthError::Discovery(msg) => assert!(
            msg.contains("http sem TLS"),
            "mensagem não explica o motivo: {msg}"
        ),
        outro => panic!("esperava recusa de transporte, veio {outro:?}"),
    }
}

#[tokio::test]
async fn emissor_que_nao_responde_falha_na_construcao() {
    // Porta fechada: a construção precisa falhar, e não devolver um
    // verificador que recusa tudo em runtime.
    let err = JwksAuth::discover("http://127.0.0.1:1/").await.unwrap_err();

    assert!(matches!(err, AuthError::Discovery(_)), "veio {err:?}");
}

// ---------------------------------------------------------------------------
// Escape hatch de transporte
// ---------------------------------------------------------------------------

/// `from_jwks_json` existe para quem já tem cliente HTTP configurado e não
/// quer a feature `jwks` no binário.
#[test]
fn from_jwks_json_monta_o_verificador_sem_rede() {
    let auth = JwksAuth::from_jwks_json(JWKS)
        .expect("JWKS válido")
        .issuer("https://idp.exemplo.com/");

    assert!(auth.has_key(KID));

    let token = token_rs256(Some(KID), &claims("https://idp.exemplo.com/"));
    assert!(auth.verify::<StandardClaims>(&token).is_ok());
}

#[test]
fn jwks_sem_nenhuma_chave_utilizavel_falha_na_construcao() {
    let err = JwksAuth::from_jwks_json(r#"{"keys":[]}"#).unwrap_err();

    assert!(
        matches!(err, AuthError::InvalidKey(_)),
        "JWKS vazio precisa falhar cedo: um verificador sem chave recusaria \
         toda requisição em runtime, longe da causa — erro: {err:?}"
    );
}

/// Silenciar `unused` no módulo comum, que este teste não usa por inteiro.
#[allow(dead_code)]
fn _usa_common() {
    let _ = common::SEGREDO;
}

// ---------------------------------------------------------------------------
// Ponta a ponta pelo framework
// ---------------------------------------------------------------------------

/// O `JwksAuth` precisa atravessar o `AuthLayer` e o default deny como o
/// `JwtAuth` atravessa. Sem isto, os testes acima provariam só que o
/// verificador funciona sozinho — que foi exatamente o vão que deixou o
/// `AuthGate` inerte por um tempo.
#[tokio::test]
async fn jwks_ativa_o_default_deny_pelo_app() {
    use axum::body::Body;
    use http::{Request as HttpRequest, StatusCode};
    use serverust_auth::AuthLayer;
    use serverust_core::App;
    use serverust_macros::get as get_route;
    use tower::ServiceExt;

    #[get_route("/relatorio")]
    async fn relatorio() -> &'static str {
        "dados-confidenciais"
    }

    let issuer = subir_idp(documento_padrao, JWKS).await;
    let auth = JwksAuth::discover(&issuer).await.expect("descoberta");

    let router = App::new()
        .without_docs()
        .auth(AuthLayer::<StandardClaims, _>::new(auth))
        .route(relatorio)
        .into_router();

    let sem_token = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/relatorio")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        sem_token.status(),
        StatusCode::UNAUTHORIZED,
        "o AuthLayer com JwksAuth não ativou o default deny"
    );

    let token = token_rs256(Some(KID), &claims(&issuer));
    let com_token = router
        .oneshot(
            HttpRequest::builder()
                .uri("/relatorio")
                .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(com_token.status(), StatusCode::OK);
}
