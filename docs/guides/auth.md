# Guia de autenticação com serverust-auth

Este guia mostra como proteger rotas verificando JWT emitido por um provedor de identidade externo — Cognito, Auth0, Clerk, Keycloak, Logto.

O `serverust-auth` **não** implementa login, hash de senha nem sessão. Isso exige estado e banco, encaixa mal em Lambda e é território de crates como `torii` e `axum-login`. O raciocínio completo está na [ADR 0009](../development/decisions/0009-auth-authz-crate-separada-serverust-auth.md).

## Pré-requisitos

- Projeto serverust rodando (ver [getting-started.md](getting-started.md)).
- Um IdP emitindo JWT, ou um segredo/chave pública conhecido no boot.

## Setup de dependências

```toml
[dependencies]
serverust-auth = "0.4"
serverust-core = "0.4"
serverust-macros = "0.4"
```

O crate é opt-in: quem não o declara não paga cripto em binário nem em cold start. O backend é Rust puro (feature `rust_crypto` do `jsonwebtoken`), escolhido para não exigir cmake nem toolchain C — é o que mantém funcionando a cross-compilação x86_64 → aarch64 do build ARM64 de Lambda.

## Como as peças se encaixam

```text
AuthLayer  valida o token UMA vez e deposita o resultado nas extensions
    │
    ├── Auth<C>       extractor tipado — exige identidade, 401 sem ela
    └── MaybeAuth<C>  extractor opcional — identidade quando houver
```

O layer **nunca rejeita**: ele só registra o que encontrou (as claims, ou o `AuthError` correspondente). Quem transforma isso em 401 é o extractor `Auth<C>`. A separação existe porque o layer roda em todas as rotas, inclusive nas públicas — rejeitar ali impediria uma rota pública de responder.

Validar no layer e ler no extractor também evita revalidar o mesmo token em cada peça do pipeline que precise dele.

## Construindo o verificador

`JwtAuth` cobre o caso de **chave estática**: segredo simétrico vindo de variável de ambiente ou Secrets Manager, ou chave pública embutida no binário. Não faz I/O — a verificação é síncrona e não toca a rede.

```rust
use serverust_auth::JwtAuth;

// HMAC-SHA256 com segredo compartilhado
let auth = JwtAuth::hs256(b"segredo")
    .issuer("https://idp.exemplo.com/")
    .audience("minha-api")
    .leeway(30); // tolerância em segundos para desalinhamento de relógio

// RSA-SHA256 com chave pública em PEM
let auth = JwtAuth::rs256_pem(include_bytes!("../keys/idp.pem"))?;

// ECDSA P-256 com chave pública em PEM
let auth = JwtAuth::es256_pem(include_bytes!("../keys/idp-ec.pem"))?;
```

O algoritmo é fixado no construtor e **não** é lido do header do token. Isso fecha a classe de ataque de confusão de algoritmo, em que um token forjado declara `alg: HS256` para que a chave pública RSA do servidor seja usada como segredo HMAC.

A validação já exige a claim `exp` e checa expiração por default. `issuer`, `audience` e `leeway` são opcionais e acumulativos.

## Instalando o layer

Use `App::layer` — o mesmo ponto de extensão de qualquer `tower::Layer`:

```rust
use serverust_auth::{AuthLayer, JwtAuth, StandardClaims};
use serverust_core::App;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET");
    let auth = JwtAuth::hs256(secret.as_bytes()).issuer("https://idp.exemplo.com/");

    App::new()
        .layer(AuthLayer::<StandardClaims>::new(auth))
        .route(me)
        .route(health)
        .run_http("127.0.0.1:3000")
        .await
}
```

Se o mesmo `JwtAuth` alimentar mais de um ponto da aplicação, use `AuthLayer::shared(Arc<JwtAuth>)` em vez de `new`.

## Handlers

Pedir `Auth<C>` na assinatura é o que protege a rota: sem token válido o extractor devolve 401 e o corpo do handler não executa.

```rust
use serverust_auth::{Auth, MaybeAuth, StandardClaims};
use serverust_macros::get;

#[get("/me")]
async fn me(user: Auth<StandardClaims>) -> String {
    format!("olá, {}", user.sub)
}

// Sem `Auth` na assinatura, a rota responde a todo mundo.
#[get("/health")]
async fn health() -> &'static str {
    "ok"
}

// Muda de comportamento com usuário autenticado, mas segue respondendo sem ele.
#[get("/feed")]
async fn feed(user: MaybeAuth<StandardClaims>) -> String {
    match user.0 {
        Some(claims) => format!("feed de {}", claims.sub),
        None => "feed público".into(),
    }
}
```

`Auth<C>` implementa `Deref` para `C`, então as claims são lidas direto (`user.sub`). `MaybeAuth<C>` expõe um `Option<Arc<C>>` no campo `.0` e nunca rejeita.

Se o `AuthLayer` não estiver instalado, `Auth<C>` nega: uma rota que pede identidade sem autenticação configurada falha fechada.

## Claims

`StandardClaims` cobre os formatos mais comuns de OAuth 2.0 e OIDC:

| Campo | Tipo | Uso |
|---|---|---|
| `sub` | `String` | identificador do principal |
| `iss` | `Option<String>` | emissor |
| `exp` | `Option<u64>` | expiração, em segundos desde a época Unix |
| `scope` | `Option<String>` | escopos separados por espaço (RFC 6749) |
| `scp` | `Vec<String>` | escopos em array, formato de alguns emissores |
| `roles` | `Vec<String>` | papéis |
| `groups` | `Vec<String>` | grupos, tratados como papéis |

### Tipo de claims próprio

Formatos fora do que `StandardClaims` cobre pedem um tipo do seu domínio implementando `AuthzFacts` — é o caminho esperado, não um plano B. Qualquer tipo que seja `DeserializeOwned` + `AuthzFacts` serve como `C`.

```rust
use serde::Deserialize;
use serverust_auth::AuthzFacts;

#[derive(Debug, Deserialize)]
struct MinhasClaims {
    sub: String,
    tenant_id: String,
    #[serde(default)]
    permissions: Vec<String>,
}

impl AuthzFacts for MinhasClaims {
    fn subject(&self) -> &str {
        &self.sub
    }

    fn has_scope(&self, scope: &str) -> bool {
        self.permissions.iter().any(|p| p == scope)
    }
}
```

`has_scope` e `has_role` têm default `false`: um tipo de claims que só carrega identidade nega toda autorização em vez de concedê-la por omissão. Implemente apenas o que o seu token realmente expressa.

Depois é só trocar o parâmetro de tipo: `AuthLayer::<MinhasClaims>::new(auth)` e `Auth<MinhasClaims>` nos handlers.

## Respostas de erro

O 401 devolvido carrega `WWW-Authenticate: Bearer` (RFC 6750 §3) e um corpo JSON:

```json
{ "error": "unauthorized", "reason": "token_expired" }
```

O campo `reason` é código estável e **faz parte da API pública** — clientes podem ramificar sobre ele sem depender da mensagem.

| `reason` | Quando ocorre |
|---|---|
| `missing_credentials` | nenhum header `Authorization` presente |
| `malformed_authorization_header` | header fora do formato `Authorization: Bearer <token>` |
| `token_expired` | assinatura válida, mas `exp` no passado |
| `invalid_issuer` | claim `iss` diferente do emissor esperado |
| `invalid_audience` | claim `aud` diferente da audiência esperada |
| `invalid_token` | assinatura inválida, algoritmo divergente ou payload indesserializável |
| `invalid_key` | falha ao construir o verificador a partir do material de chave — ocorre na inicialização, não no caminho de request |

O esquema `Bearer` é comparado sem diferenciar maiúsculas, como manda a RFC 7235 §2.1.

## Default deny: instalar o layer protege todas as rotas

**Instalar o `AuthLayer` ativa o default deny.** A partir daí, toda rota exige identidade válida — inclusive as que **não** pedem `Auth<C>` na assinatura:

```rust
// Protegida, mesmo sem Auth<C> na assinatura.
#[get("/relatorio")]
async fn relatorio() -> &'static str { "dados" }
```

Quem aplica isso é o `AuthGate` do `serverust-core`, que o `App::route()` coloca em toda rota não marcada como pública. O layer nunca rejeita sozinho: ele registra nas extensions se há autenticação instalada (`AuthEnabled`), se a requisição trouxe identidade válida (`Authenticated`) e, quando falhou, o motivo (`AuthFailure`). O portão lê esses marcadores e decide.

O motivo preciso atravessa a rejeição: uma rota protegida só pelo default deny ainda devolve `token_expired` ou `invalid_issuer`, não um genérico. Só quando não há motivo registrado a resposta é `authentication_required`.

### Abrindo uma rota

A exceção é explícita e auditável — `grep` acha marcação, nunca a falta dela. Enquanto a macro `#[public]` não existe, use a via programática:

```rust
use serverust_core::{IntoRoute, Route};
use utoipa::openapi::{HttpMethod, path::Operation};

struct Health;

impl IntoRoute for Health {
    fn into_route(self) -> Route {
        Route::new("/health", HttpMethod::Get, axum::routing::get(|| async { "ok" }), Operation::new())
            .public()
    }
}
```

> **Adotando em serviço existente:** instalar o layer fecha todas as rotas de uma vez. Rode a suíte de testes — as falhas são a sua checklist do que precisa ser marcado público.

**O que o portão não alcança:** as rotas de documentação (`/openapi.json`, `/docs`, `/redoc`) não passam por `App::route()` e seguem abertas; feche-as com `App::without_docs()`. E rota registrada direto no `axum::Router`, fora do `App`, também não é embrulhada.

## O que ainda não está implementado

Esta é a primeira parcela da ADR 0009. Está fora do que existe hoje:

- **Descoberta de JWKS/OIDC.** Só há chave estática. O construtor `async` com a busca do JWKS aquecida na fase de init, e o refresh sob demanda em caso de `kid` desconhecido, vêm em incremento seguinte.
- **`#[authorize(scope = "...")]`.** A trait `AuthzFacts` já existe e o layer já publica os fatos apagados de tipo nas extensions, mas a macro que gera o `Guard` correspondente ainda não. Autorização por escopo hoje é checagem manual dentro do handler.
- **Marcar rota pública por macro.** O default deny já vale (veja abaixo), mas a macro `#[public]` ainda não existe. Enquanto isso, a única forma de declarar uma rota aberta é a via programática `Route::public()`, implementando `IntoRoute` à mão.
- **`security` automático no OpenAPI.** O botão "Authorize" do Scalar/Swagger UI ainda precisa de configuração manual.

## Veja também

- [`serverust-auth/src/jwt.rs`](../../serverust-auth/src/jwt.rs) — verificador e extração do header `Authorization`.
- [`serverust-auth/src/layer.rs`](../../serverust-auth/src/layer.rs) — o layer e o service que ele produz.
- [`serverust-auth/src/claims.rs`](../../serverust-auth/src/claims.rs) — `AuthzFacts`, `Claims` e `StandardClaims`.
- [ADR 0009](../development/decisions/0009-auth-authz-crate-separada-serverust-auth.md) — o desenho completo, incluindo o que está planejado.
