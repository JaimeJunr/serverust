//! Revalidação do JWKS sob demanda.
//!
//! O IdP falso aqui é **mutável e contador**: o teste troca o JWKS que ele
//! serve, e conta quantas vezes foi buscado. Sem o contador, os testes de
//! limite de frequência e de single-flight não provariam nada — passariam com
//! qualquer número de buscas.

#![cfg(feature = "jwks")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, get_current_timestamp};
use serde_json::{Value, json};
use serverust_auth::{AuthError, JwksAuth, StandardClaims, Verifier};
use tokio::sync::RwLock;

const KID_ORIGINAL: &str = "chave-de-teste-1";
const KID_ROTACIONADO: &str = "chave-de-teste-2";

const JWKS_ORIGINAL: &str = include_str!("fixtures/rs256_jwks.json");
const JWKS_ROTACIONADO: &str = include_str!("fixtures/rs256_jwks_rotacionado.json");

const CHAVE_ORIGINAL: &[u8] = include_bytes!("fixtures/rs256_private.pem");
const CHAVE_ROTACIONADA: &[u8] = include_bytes!("fixtures/rs256_rotacionada_private.pem");

// ---------------------------------------------------------------------------
// IdP falso, mutável e contador
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Idp {
    issuer: String,
    jwks: Arc<RwLock<String>>,
    buscas: Arc<AtomicUsize>,
}

impl Idp {
    async fn subir() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("porta efêmera");
        let addr: SocketAddr = listener.local_addr().expect("endereço local");
        let issuer = format!("http://127.0.0.1:{}", addr.port());

        let idp = Self {
            issuer: issuer.clone(),
            jwks: Arc::new(RwLock::new(JWKS_ORIGINAL.to_string())),
            buscas: Arc::new(AtomicUsize::new(0)),
        };

        let doc = json!({ "issuer": issuer, "jwks_uri": format!("{issuer}/jwks") });
        let servir_jwks = {
            let idp = idp.clone();
            move || {
                let idp = idp.clone();
                async move {
                    idp.buscas.fetch_add(1, Ordering::SeqCst);
                    let corpo = idp.jwks.read().await.clone();
                    ([(http::header::CONTENT_TYPE, "application/json")], corpo)
                }
            }
        };

        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(move || {
                    let doc = doc.clone();
                    async move { axum::Json(doc) }
                }),
            )
            .route("/jwks", get(servir_jwks));

        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("servidor do IdP");
        });

        idp
    }

    /// Troca o JWKS servido, como faria um emissor ao rotacionar.
    async fn rotacionar(&self) {
        *self.jwks.write().await = JWKS_ROTACIONADO.to_string();
    }

    fn buscas(&self) -> usize {
        self.buscas.load(Ordering::SeqCst)
    }
}

fn token(kid: &str, chave: &[u8], issuer: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_string());
    encode(
        &header,
        &json!({ "sub": "user-42", "iss": issuer, "exp": get_current_timestamp() + 3600 }),
        &EncodingKey::from_rsa_pem(chave).expect("chave privada de teste"),
    )
    .expect("assinatura")
}

fn token_expirado(kid: &str, chave: &[u8], issuer: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_string());
    encode(
        &header,
        &json!({ "sub": "user-42", "iss": issuer, "exp": get_current_timestamp() - 7200 }),
        &EncodingKey::from_rsa_pem(chave).expect("chave privada de teste"),
    )
    .expect("assinatura")
}

/// `Verifier::revalidate` precisa da trait em escopo; esta função existe para
/// que os testes chamem o caminho lento como o layer chama.
async fn verificar_com_revalidacao(
    auth: &JwksAuth,
    token: &str,
) -> Result<StandardClaims, AuthError> {
    match Verifier::verify::<StandardClaims>(auth, token) {
        Ok(claims) => Ok(claims),
        Err(err) if auth.can_revalidate(&err) => {
            auth.revalidate::<StandardClaims>(token, err).await
        }
        Err(err) => Err(err),
    }
}

// ---------------------------------------------------------------------------
// Rotação
// ---------------------------------------------------------------------------

/// O caso que motiva a feature: o emissor rotacionou depois de este processo
/// ter carregado o JWKS, e o serviço se recupera sozinho.
#[tokio::test]
async fn rotacao_do_emissor_se_resolve_sozinha() {
    let idp = Idp::subir().await;
    let auth = JwksAuth::discover(&idp.issuer).await.expect("descoberta");
    assert_eq!(idp.buscas(), 1, "a descoberta busca o JWKS uma vez");

    // Antes da rotação: a chave nova é desconhecida.
    idp.rotacionar().await;
    let novo = token(KID_ROTACIONADO, CHAVE_ROTACIONADA, &idp.issuer);

    assert_eq!(
        Verifier::verify::<StandardClaims>(&auth, &novo).unwrap_err(),
        AuthError::UnknownKeyId,
        "sem revalidar, a chave rotacionada é desconhecida"
    );

    // Com revalidação: rebusca e passa.
    let claims = verificar_com_revalidacao(&auth, &novo)
        .await
        .expect("a revalidação deveria ter trazido a chave nova");

    assert_eq!(claims.sub, "user-42");
    assert_eq!(idp.buscas(), 2, "uma rebusca, e só uma");
    assert!(auth.has_key(KID_ROTACIONADO));
}

/// A revalidação troca o mapa inteiro: chave retirada pelo emissor para de
/// valer. É o outro lado da rotação, e o que impede uma chave comprometida de
/// sobreviver à retirada.
#[tokio::test]
async fn chave_retirada_pelo_emissor_deixa_de_valer() {
    let idp = Idp::subir().await;
    let auth = JwksAuth::discover(&idp.issuer).await.expect("descoberta");

    idp.rotacionar().await;
    let novo = token(KID_ROTACIONADO, CHAVE_ROTACIONADA, &idp.issuer);
    verificar_com_revalidacao(&auth, &novo)
        .await
        .expect("rotação");

    let antigo = token(KID_ORIGINAL, CHAVE_ORIGINAL, &idp.issuer);
    assert_eq!(
        verificar_com_revalidacao(&auth, &antigo).await.unwrap_err(),
        AuthError::UnknownKeyId,
        "a chave que o emissor retirou continuou aceita"
    );
    assert!(!auth.has_key(KID_ORIGINAL));
}

// ---------------------------------------------------------------------------
// Limite de frequência — o controle de segurança
// ---------------------------------------------------------------------------

/// Sem limite, qualquer um força uma busca de JWKS por requisição, inventando
/// um `kid`. O serviço viraria amplificador contra o próprio emissor.
#[tokio::test]
async fn kid_inventado_nao_busca_mais_de_uma_vez_por_janela() {
    let idp = Idp::subir().await;
    let auth = JwksAuth::discover(&idp.issuer).await.expect("descoberta");
    let depois_da_descoberta = idp.buscas();

    // Vinte requisições de atacante, cada uma com um `kid` diferente.
    for i in 0..20 {
        let forjado = token(&format!("kid-inventado-{i}"), CHAVE_ORIGINAL, &idp.issuer);
        assert_eq!(
            verificar_com_revalidacao(&auth, &forjado)
                .await
                .unwrap_err(),
            AuthError::UnknownKeyId
        );
    }

    assert_eq!(
        idp.buscas() - depois_da_descoberta,
        1,
        "20 requisições com kid inventado renderam mais de uma busca: o \
         intervalo mínimo não está limitando, e o serviço amplifica contra o IdP"
    );
}

/// Passada a janela, uma rotação legítima volta a ser detectável.
#[tokio::test]
async fn passada_a_janela_a_revalidacao_volta_a_acontecer() {
    let idp = Idp::subir().await;
    let auth = JwksAuth::discover(&idp.issuer)
        .await
        .expect("descoberta")
        .revalidation_interval(Duration::from_millis(50));

    let forjado = token("kid-inventado", CHAVE_ORIGINAL, &idp.issuer);
    let _ = verificar_com_revalidacao(&auth, &forjado).await;
    let depois_da_primeira = idp.buscas();

    tokio::time::sleep(Duration::from_millis(80)).await;

    idp.rotacionar().await;
    let novo = token(KID_ROTACIONADO, CHAVE_ROTACIONADA, &idp.issuer);
    verificar_com_revalidacao(&auth, &novo)
        .await
        .expect("passada a janela, a rotação precisa ser detectada");

    assert_eq!(idp.buscas(), depois_da_primeira + 1);
}

/// Erro que chave nova não conserta não pode disparar I/O. Token expirado
/// continuaria expirado depois da rebusca.
#[tokio::test]
async fn erro_que_chave_nova_nao_conserta_nao_busca() {
    let idp = Idp::subir().await;
    let auth = JwksAuth::discover(&idp.issuer).await.expect("descoberta");
    let depois_da_descoberta = idp.buscas();

    let expirado = token_expirado(KID_ORIGINAL, CHAVE_ORIGINAL, &idp.issuer);
    assert_eq!(
        verificar_com_revalidacao(&auth, &expirado)
            .await
            .unwrap_err(),
        AuthError::Expired
    );

    let outro_emissor = token(KID_ORIGINAL, CHAVE_ORIGINAL, "https://outro.exemplo/");
    assert_eq!(
        verificar_com_revalidacao(&auth, &outro_emissor)
            .await
            .unwrap_err(),
        AuthError::InvalidIssuer
    );

    assert_eq!(
        idp.buscas(),
        depois_da_descoberta,
        "token expirado ou de outro emissor disparou rebusca — é I/O \
         garantidamente inútil, alcançável por qualquer requisição"
    );
}

// ---------------------------------------------------------------------------
// Single-flight
// ---------------------------------------------------------------------------

/// N requisições concorrentes com o mesmo `kid` desconhecido precisam render
/// uma busca, não N. Sem isso, uma rotação num serviço sob carga produz uma
/// rajada contra o emissor no exato momento em que ele acabou de mudar.
#[tokio::test]
async fn revalidacoes_concorrentes_rendem_uma_busca() {
    let idp = Idp::subir().await;
    let auth = Arc::new(JwksAuth::discover(&idp.issuer).await.expect("descoberta"));
    let depois_da_descoberta = idp.buscas();

    idp.rotacionar().await;
    let novo = token(KID_ROTACIONADO, CHAVE_ROTACIONADA, &idp.issuer);

    let mut tarefas = Vec::new();
    for _ in 0..16 {
        let auth = Arc::clone(&auth);
        let novo = novo.clone();
        tarefas.push(tokio::spawn(async move {
            verificar_com_revalidacao(&auth, &novo).await
        }));
    }

    for t in tarefas {
        t.await
            .expect("tarefa")
            .expect("toda requisição concorrente deveria passar após a rebusca");
    }

    assert_eq!(
        idp.buscas() - depois_da_descoberta,
        1,
        "16 requisições concorrentes renderam mais de uma busca: o mutex não \
         está serializando a revalidação"
    );
}

// ---------------------------------------------------------------------------
// Quem não tem de onde rebuscar
// ---------------------------------------------------------------------------

/// `from_jwks_json` não guarda URL — quem trouxe o próprio transporte não nos
/// deu jeito de repetir a busca. O verificador se comporta como antes desta
/// capacidade existir.
#[tokio::test]
async fn from_jwks_json_nao_revalida() {
    let auth = JwksAuth::from_jwks_json(JWKS_ORIGINAL)
        .expect("JWKS válido")
        .issuer("https://idp.exemplo.com/");

    assert!(
        !auth.can_revalidate(&AuthError::UnknownKeyId),
        "sem jwks_uri não há o que rebuscar"
    );
}

/// Chave estática nunca revalida: quem não usa JWKS não paga por esta
/// capacidade, nem em I/O nem em comportamento.
#[test]
fn jwt_auth_nunca_revalida() {
    use serverust_auth::JwtAuth;

    let auth = JwtAuth::hs256(b"segredo");
    assert!(!auth.can_revalidate(&AuthError::UnknownKeyId));
    assert!(!auth.can_revalidate(&AuthError::Expired));
}

// ---------------------------------------------------------------------------
// Pelo framework
// ---------------------------------------------------------------------------

/// A prova que justifica tudo acima: a revalidação atravessa o `AuthLayer`.
///
/// Os testes anteriores chamam `can_revalidate`/`revalidate` como o layer
/// chamaria. Este monta um `App` de verdade e deixa o layer decidir — é a
/// diferença entre provar que as peças funcionam e provar que estão ligadas,
/// que é exatamente o vão que deixou o `AuthGate` inerte por um tempo.
#[tokio::test]
async fn rotacao_se_resolve_atraves_do_authlayer() {
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

    let idp = Idp::subir().await;
    let auth = JwksAuth::discover(&idp.issuer).await.expect("descoberta");

    let router = App::new()
        .without_docs()
        .auth(AuthLayer::<StandardClaims, _>::new(auth))
        .route(relatorio)
        .into_router();

    let pedir = |token: String| {
        let router = router.clone();
        async move {
            router
                .oneshot(
                    HttpRequest::builder()
                        .uri("/relatorio")
                        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }
    };

    // Antes da rotação, o caminho rápido resolve.
    assert_eq!(
        pedir(token(KID_ORIGINAL, CHAVE_ORIGINAL, &idp.issuer)).await,
        StatusCode::OK
    );
    assert_eq!(idp.buscas(), 1, "o caminho rápido não pode buscar nada");

    // O emissor rotaciona. A primeira requisição com a chave nova precisa
    // passar — é o layer que tem de tomar o caminho lento.
    idp.rotacionar().await;
    assert_eq!(
        pedir(token(KID_ROTACIONADO, CHAVE_ROTACIONADA, &idp.issuer)).await,
        StatusCode::OK,
        "o AuthLayer não acionou a revalidação — a rotação produziria 401 até \
         o container reciclar, que é exatamente o que esta feature veio corrigir"
    );
    assert_eq!(idp.buscas(), 2, "uma rebusca");

    // E o caminho rápido volta a resolver com a chave nova, sem mais I/O.
    assert_eq!(
        pedir(token(KID_ROTACIONADO, CHAVE_ROTACIONADA, &idp.issuer)).await,
        StatusCode::OK
    );
    assert_eq!(
        idp.buscas(),
        2,
        "depois da rebusca, a chave nova está no mapa e o caminho rápido basta"
    );
}

/// Token inválido por motivo que chave nova não conserta precisa continuar
/// sendo 401 barato, sem I/O, mesmo atravessando o layer.
#[tokio::test]
async fn token_sem_credencial_nao_busca_atraves_do_layer() {
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

    let idp = Idp::subir().await;
    let auth = JwksAuth::discover(&idp.issuer).await.expect("descoberta");
    let depois_da_descoberta = idp.buscas();

    let router = App::new()
        .without_docs()
        .auth(AuthLayer::<StandardClaims, _>::new(auth))
        .route(relatorio)
        .into_router();

    let resp = router
        .oneshot(
            HttpRequest::builder()
                .uri("/relatorio")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        idp.buscas(),
        depois_da_descoberta,
        "requisição sem credencial disparou I/O contra o emissor"
    );
}

/// Silencia o aviso de item não usado.
#[allow(dead_code)]
fn _ignora(_: Value) {}
