//! `security` automático no OpenAPI (ADR 0009, etapa 5).
//!
//! O que estes testes guardam não é a forma do JSON — é a promessa de que o
//! documento e o portão dizem a mesma coisa. Um documento que descreve como
//! aberta uma rota que o `AuthGate` fecha (ou o contrário) é pior do que
//! documento nenhum, porque alguém confia nele.

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use http::{Method, Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use serverust_core::{App, AuthEnabled, Interceptor, IntoRoute, Route};
use serverust_macros::{authorize, get as get_route, public};
use tower::ServiceExt;
use utoipa::openapi::HttpMethod;
use utoipa::openapi::path::Operation;

// ---------------------------------------------------------------------------
// Apoio
// ---------------------------------------------------------------------------

/// Layer que não faz nada, só serve para o `App::auth` registrar que há
/// autenticação instalada — que é o único bit que o documento consulta.
///
/// O comportamento em runtime é exercitado pelo [`SoLigaAuth`], separado de
/// propósito: assim o teste de concordância compara duas coisas construídas
/// independentemente, em vez de duas vistas do mesmo dublê.
#[derive(Clone, Copy)]
struct MarcaAuthInstalada;

impl<S> tower::Layer<S> for MarcaAuthInstalada {
    type Service = S;

    fn layer(&self, inner: S) -> S {
        inner
    }
}

#[public]
#[get_route("/health")]
async fn health() -> &'static str {
    "ok"
}

#[get_route("/relatorio")]
async fn relatorio() -> &'static str {
    "confidencial"
}

#[authorize(scope = "orders:read")]
#[get_route("/pedidos")]
async fn pedidos() -> &'static str {
    "pedidos"
}

#[authorize(scope = "orders:read", role = "admin")]
#[authorize(scope = "orders:write")]
#[get_route("/pedidos/admin")]
async fn pedidos_admin() -> &'static str {
    "admin"
}

/// Rota pública pela via programática, para conferir que as duas formas de
/// marcar caem no mesmo lugar do documento.
struct PingProgramatico;

impl IntoRoute for PingProgramatico {
    fn into_route(self) -> Route {
        Route::new(
            "/ping",
            HttpMethod::Get,
            get(|| async { "pong" }),
            Operation::new(),
        )
        .public()
    }
}

fn app_com_auth() -> App {
    App::new()
        .auth(MarcaAuthInstalada)
        .route(health)
        .route(relatorio)
        .route(pedidos)
        .route(pedidos_admin)
        .route(PingProgramatico)
}

async fn documento(app: App) -> Value {
    let router = app.into_router();
    let resp = router
        .oneshot(
            HttpRequest::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("o /openapi.json precisa ser JSON válido")
}

fn security_de(doc: &Value, path: &str) -> Value {
    doc["paths"][path]["get"]["security"].clone()
}

// ---------------------------------------------------------------------------
// Esquema de segurança
// ---------------------------------------------------------------------------

#[tokio::test]
async fn com_auth_o_documento_declara_o_esquema_bearer() {
    let doc = documento(app_com_auth()).await;
    let esquema = &doc["components"]["securitySchemes"]["bearerAuth"];

    assert_eq!(esquema["type"], "http", "esquema: {esquema}");
    assert_eq!(esquema["scheme"], "bearer", "esquema: {esquema}");
    assert_eq!(esquema["bearerFormat"], "JWT", "esquema: {esquema}");
}

/// Sem autenticação instalada, o documento não pode falar de segurança:
/// declarar um esquema descreveria uma exigência que não existe.
#[tokio::test]
async fn sem_auth_o_documento_nao_fala_de_seguranca() {
    let doc = documento(App::new().route(relatorio).route(pedidos)).await;

    assert!(
        doc["components"]["securitySchemes"].is_null(),
        "documento sem auth declarou esquema de segurança: {doc}"
    );
    assert!(doc["security"].is_null(), "documento: {doc}");
    assert!(
        security_de(&doc, "/pedidos").is_null(),
        "operação: {}",
        security_de(&doc, "/pedidos")
    );
}

// ---------------------------------------------------------------------------
// Exigência por rota
// ---------------------------------------------------------------------------

#[tokio::test]
async fn o_default_do_documento_e_exigir_credencial() {
    let doc = documento(app_com_auth()).await;

    assert_eq!(
        doc["security"],
        serde_json::json!([{ "bearerAuth": [] }]),
        "o requisito global espelha o default deny — documento: {}",
        doc["security"]
    );
}

#[tokio::test]
async fn rota_protegida_exige_o_esquema() {
    let doc = documento(app_com_auth()).await;

    assert_eq!(
        security_de(&doc, "/relatorio"),
        serde_json::json!([{ "bearerAuth": [] }])
    );
}

/// `security: []` é como o OpenAPI diz "esta operação não exige nada",
/// sobrescrevendo o requisito global. Ausência do campo herdaria o global e
/// faria o documento afirmar que a rota pública é protegida.
#[tokio::test]
async fn rota_publica_zera_a_exigencia() {
    let doc = documento(app_com_auth()).await;

    for path in ["/health", "/ping"] {
        assert_eq!(
            security_de(&doc, path),
            serde_json::json!([]),
            "{path} não foi declarada aberta no documento"
        );
    }
}

#[tokio::test]
async fn authorize_leva_o_escopo_para_o_documento() {
    let doc = documento(app_com_auth()).await;

    assert_eq!(
        security_de(&doc, "/pedidos"),
        serde_json::json!([{ "bearerAuth": ["orders:read"] }])
    );
}

/// Anotações empilhadas somam escopos, na ordem em que aparecem. Papéis não
/// entram: a lista do OpenAPI é uma só, e misturar as duas coisas produziria
/// um documento em que ninguém distingue escopo de papel.
#[tokio::test]
async fn escopos_empilhados_somam_e_papeis_ficam_de_fora() {
    let doc = documento(app_com_auth()).await;

    assert_eq!(
        security_de(&doc, "/pedidos/admin"),
        serde_json::json!([{ "bearerAuth": ["orders:read", "orders:write"] }]),
        "o papel `admin` vazou para a lista de escopos, ou a ordem mudou"
    );
}

// ---------------------------------------------------------------------------
// A promessa que importa
// ---------------------------------------------------------------------------

/// O documento e o portão precisam concordar: toda rota que o documento
/// declara aberta responde sem credencial, e toda rota que ele declara
/// protegida é negada.
///
/// É esta asserção que justifica a feature. As outras conferem a forma do
/// JSON; esta confere que o JSON não mente.
#[tokio::test]
async fn o_documento_concorda_com_o_portao() {
    let doc = documento(app_com_auth()).await;

    // Sem o interceptor de identidade: só as públicas devem responder.
    let router = App::new()
        .auth(MarcaAuthInstalada)
        .interceptor(SoLigaAuth)
        .route(health)
        .route(relatorio)
        .route(pedidos)
        .route(pedidos_admin)
        .route(PingProgramatico)
        .into_router();

    let paths = doc["paths"].as_object().expect("paths");
    assert_eq!(
        paths.len(),
        5,
        "o teste itera sobre os paths do documento; se a contagem mudar sem \
         alguém revisar, ele pode virar vazio em silêncio — paths: {paths:?}"
    );

    for (path, security) in paths
        .iter()
        .map(|(p, m)| (p.clone(), m["get"]["security"].clone()))
    {
        let resp = router
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri(&path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let declarada_aberta = security == serde_json::json!([]);
        let respondeu = resp.status() == StatusCode::OK;

        assert_eq!(
            declarada_aberta,
            respondeu,
            "{path}: o documento diz security={security}, mas a resposta sem \
             credencial foi {}",
            resp.status()
        );
    }
}

/// Interceptor que liga o default deny sem nunca autenticar — é o cenário do
/// cliente anônimo.
#[derive(Clone, Copy)]
struct SoLigaAuth;

impl Interceptor for SoLigaAuth {
    async fn intercept(&self, mut req: Request, next: Next) -> Response {
        req.extensions_mut().insert(AuthEnabled);
        next.run(req).await
    }
}
