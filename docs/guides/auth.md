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
use serverust_macros::{get, public};

#[get("/me")]
async fn me(user: Auth<StandardClaims>) -> String {
    format!("olá, {}", user.sub)
}

// Atenção: sem `Auth` na assinatura a rota NÃO fica aberta — com o layer
// instalado ela é protegida pelo default deny. Abrir exige `#[public]`.
#[public]
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

A exceção é explícita e auditável — `grep` acha marcação, nunca a falta dela. `grep -rn '#\[public\]' src/` lista todo endpoint anônimo do serviço, e essa lista é completa por construção.

```rust
use serverust_macros::{get, public};

#[public]
#[get("/health")]
async fn health() -> &'static str {
    "ok"
}
```

`#[public]` vem **acima** da macro de rota, como `#[guard]`. Abaixo ela não teria efeito — a rota já teria sido construída — então isso **não compila**: uma rota que se diz pública sem ser é a falha silenciosa que o default deny existe para evitar.

A via programática continua disponível, para quem constrói a `Route` à mão:

```rust
Route::new("/health", HttpMethod::Get, axum::routing::get(|| async { "ok" }), Operation::new())
    .public()
```

> **Adotando em serviço existente:** instalar o layer fecha todas as rotas de uma vez. Rode a suíte de testes — as falhas são a sua checklist do que precisa ser marcado público.

## Autorização por escopo e papel

Autenticação diz *quem* é; autorização diz *o que pode*. `#[authorize]` cobre a segunda, lendo os fatos que o `AuthLayer` publicou:

```rust
use serverust_macros::{authorize, get, post};

#[authorize(scope = "orders:read")]
#[get("/orders")]
async fn listar() -> &'static str { "pedidos" }

#[authorize(scope = "orders:write", role = "admin")]
#[post("/orders")]
async fn criar() -> &'static str { "criado" }
```

`scope` e `role` são repetíveis, e **todos** os valores listados são exigidos. Empilhar `#[authorize]` também conjunta:

```rust
#[authorize(scope = "a")]
#[authorize(scope = "b")]   // exige a E b
```

Exigência alternativa (`any_of`) não existe hoje, de propósito: um "qualquer um destes" ambíguo é o tipo de default que a [filosofia do projeto](../product/philosophy.md#o-corolário-defaults-na-era-dos-agentes) pede para não existir. Enquanto isso, um caso genuinamente alternativo cabe num `#[guard]` escrito à mão.

Ao contrário de `#[public]`, `#[authorize]` funciona acima ou abaixo da macro de rota.

`#[public]` e `#[authorize]` na mesma rota **não compilam**: `#[authorize]` já nega sem identidade, então o `#[public]` não abriria nada — só faria a rota aparecer na auditoria de endpoints anônimos sem ser um.

### Respostas

| Situação | Resposta |
|---|---|
| Sem identidade na requisição | `401` `authentication_required` |
| Identidade sem o escopo/papel | `403` `insufficient_scope` (RFC 6750 §3.1) |

O 403 tem o mesmo formato do 401 (`{"error": ..., "reason": ...}`), com `error` igual a `"forbidden"`.

A ausência de identidade nega **inclusive em rota `#[public]`** e **inclusive sem `AuthLayer` instalado**. `#[authorize]` numa aplicação sem autenticação configurada rejeita tudo em vez de liberar tudo: a combinação "pedi permissão e não tenho de onde lê-la" não pode resultar em acesso.

### De onde vêm os fatos

`#[authorize]` consulta `serverust_core::AuthzFacts` — a mesma trait que o seu tipo de claims implementa. Quem escreveu um tipo próprio (veja [Claims](#tipo-de-claims-próprio)) já está coberto: `has_scope` e `has_role` são exatamente o que a macro chama. Quem não implementou nenhum dos dois nega toda autorização, porque os defaults da trait são `false`.

**O que o portão não alcança:** as rotas de documentação (`/openapi.json`, `/docs`, `/redoc`) não passam por `App::route()` e seguem abertas; feche-as com `App::without_docs()`. E rota registrada direto no `axum::Router`, fora do `App`, também não é embrulhada.

## O que ainda não está implementado

A ADR 0009 é entregue em parcelas. Está fora do que existe hoje:

- **Descoberta de JWKS/OIDC.** Só há chave estática. O construtor `async` com a busca do JWKS aquecida na fase de init, e o refresh sob demanda em caso de `kid` desconhecido, vêm em incremento seguinte.
- **Log de inicialização listando as rotas públicas.** A anotação já é auditável por `grep`; ver a lista no boot do serviço ainda não é possível.
- **Exigência alternativa em `#[authorize]`.** Só há conjunção (AND). Um `any_of` cabe hoje num `#[guard]` escrito à mão.
- **`security` automático no OpenAPI.** O botão "Authorize" do Scalar/Swagger UI ainda precisa de configuração manual.

## Veja também

- [`serverust-auth/src/jwt.rs`](../../serverust-auth/src/jwt.rs) — verificador e extração do header `Authorization`.
- [`serverust-auth/src/layer.rs`](../../serverust-auth/src/layer.rs) — o layer e o service que ele produz.
- [`serverust-auth/src/claims.rs`](../../serverust-auth/src/claims.rs) — `AuthzFacts`, `Claims` e `StandardClaims`.
- [ADR 0009](../development/decisions/0009-auth-authz-crate-separada-serverust-auth.md) — o desenho completo, incluindo o que está planejado.
