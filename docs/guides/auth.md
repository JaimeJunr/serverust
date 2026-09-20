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

O crate é opt-in: quem não o declara não paga cripto em binário nem em cold start. A feature `jwks` (desligada por default) acrescenta cliente HTTP e pilha TLS, e só é necessária para [buscar as chaves do emissor](#chaves-do-emissor-por-jwks). O backend é Rust puro (feature `rust_crypto` do `jsonwebtoken`), escolhido para não exigir cmake nem toolchain C — é o que mantém funcionando a cross-compilação x86_64 → aarch64 do build ARM64 de Lambda.

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

### Chaves do emissor, por JWKS

Quando o IdP publica as chaves em vez de você as embutir, use `JwksAuth`. Habilite a feature:

```toml
serverust-auth = { version = "0.4", features = ["jwks"] }
```

```rust
use serverust_auth::JwksAuth;

let auth = JwksAuth::discover("https://idp.exemplo.com/")
    .await?
    .audience("minha-api");
```

`discover` busca `/.well-known/openid-configuration`, lê o `jwks_uri` e carrega as chaves. O `issuer` do documento é aplicado como emissor esperado — **validação de `iss` fica ligada sem você pedir**. Para emissores sem documento de descoberta, `JwksAuth::from_jwks_uri(url)` carrega o JWKS direto; aí o `issuer` é por sua conta.

**O construtor é `async` de propósito.** Chame no `main`, antes de montar o `App`:

```rust
#[tokio::main]
async fn main() -> std::io::Result<()> {
    let auth = JwksAuth::discover("https://idp.exemplo.com/").await.unwrap();

    App::new()
        .auth(AuthLayer::<StandardClaims, _>::new(auth))
        .route(me)
        .run_http("127.0.0.1:3000")
        .await
}
```

Em Lambda, é isso que coloca a ida à rede na **fase de init**, onde há burst de CPU e a busca sai de graça. A alternativa — buscar preguiçosamente na primeira requisição — faria cada container frio pagar a latência, e sumiria dos testes, porque em teste o container está sempre quente. Não existe construtor síncrono: esquecer de aquecer não é um erro que se possa cometer.

A escolha da chave é pelo `kid` do token, e **o algoritmo vem da chave, nunca do header do token**. Chave simétrica (`oct`) publicada num JWKS é recusada na construção: JWKS é documento público, e uma chave simétrica ali é o próprio segredo de assinatura.

Busca de JWKS por `http://` sem TLS é recusada fora de loopback — quem responder por aquela URL escolhe a chave pública que valida os tokens da sua API.

#### Trazendo o seu próprio transporte

Quem já tem cliente HTTP configurado — proxy corporativo, CA própria, credenciais de VPC — busca o JWKS com ele e entrega o corpo, sem a feature e sem cliente HTTP no binário:

```rust
let corpo = meu_cliente.get(jwks_uri).await?.text().await?;
let auth = JwksAuth::from_jwks_json(&corpo)?.issuer("https://idp.exemplo.com/");
```

Nesse caminho o aquecimento é por sua conta, porque a busca é sua.

#### Rotação de chaves

Quando o emissor rotaciona e chega um token com `kid` desconhecido, o verificador **rebusca o JWKS e tenta de novo** — a requisição que encontrou a chave nova é atendida, não rejeitada. Nada a configurar.

Isso vale só para quem construiu com `discover` ou `from_jwks_uri`: são os construtores que guardam de onde rebuscar. `from_jwks_json` não tem URL, então não revalida — a busca é sua, e repeti-la também.

**A rebusca é limitada a uma por minuto.** Não é afinação de performance, é controle de segurança: o caminho de revalidação é alcançável por requisição não autenticada — basta inventar um `kid` — e sem limite o seu serviço vira um amplificador contra o próprio IdP. Com o limite, o pior caso é uma busca por janela, independentemente do volume do ataque.

```rust
let auth = JwksAuth::discover(issuer).await?
    .revalidation_interval(Duration::from_secs(300)); // mais conservador
```

Baixar o intervalo aumenta a exposição do emissor; zerá-lo o entrega ao primeiro laço `for`.

Requisições concorrentes com o mesmo `kid` desconhecido rendem **uma** busca, não N — o que importa quando a rotação pega um serviço sob carga. E só `unknown_key_id` dispara a rebusca: token ausente, expirado, de outro emissor ou com assinatura inválida continuariam inválidos depois dela, então tentar seria I/O garantidamente inútil.

Se a rebusca não resolver — emissor fora do ar, ou dentro da janela de limite — a resposta continua sendo `401 unknown_key_id`. O erro de infraestrutura nunca vaza para o cliente: ele descreveria um problema nosso, sobre uma credencial que é dele.

O código `unknown_key_id` segue alertável, e agora significa outra coisa: não "rotação em andamento", e sim "rotação que a rebusca não resolveu" — emissor inacessível, ou `kid` que nunca existiu.

## Instalando o layer

Use `App::auth`:

```rust
use serverust_auth::{AuthLayer, JwtAuth, StandardClaims};
use serverust_core::App;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET");
    let auth = JwtAuth::hs256(secret.as_bytes()).issuer("https://idp.exemplo.com/");

    App::new()
        .auth(AuthLayer::<StandardClaims>::new(auth))
        .route(me)
        .route(health)
        .run_http("127.0.0.1:3000")
        .await
}
```

`.auth(...)` é o `.layer(...)` de sempre, com duas diferenças. A primeira é o nome: essa linha é a decisão mais consequente do serviço — a partir dela toda rota não marcada `#[public]` exige identidade — e esconder isso num ponto de extensão genérico não ajuda quem lê o `main`.

A segunda é o log de inicialização. Com `.auth(...)`, o boot imprime no stderr a superfície anônima do serviço:

```text
  🔒 serverust: default deny ativo
     2 rota(s) pública(s), sem exigir identidade:
       GET /health
       POST /webhooks/stripe
```

Lista o que é **aberto**, nunca o que é protegido: a lista curta é a que se lê, e inverter produziria um log do tamanho do serviço, que ninguém lê. `App::public_routes()` devolve o mesmo conteúdo, para afirmar a superfície anônima num teste em vez de confiar na leitura do log.

`.layer(AuthLayer::new(...))` continua funcionando e continua ativando o default deny — quem faz isso são os marcadores que o layer insere, não o método. O que se perde é a lista no boot.

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
| `unknown_key_id` | o `kid` do token não está no JWKS carregado — tipicamente rotação de chave no emissor |
| `discovery_failed` | descoberta de OIDC ou busca do JWKS falhou — ocorre na inicialização, nunca no caminho de request |

O esquema `Bearer` é comparado sem diferenciar maiúsculas, como manda a RFC 7235 §2.1.

## Default deny: instalar o layer protege todas as rotas

**Instalar autenticação ativa o default deny.** A partir daí, toda rota exige identidade válida — inclusive as que **não** pedem `Auth<C>` na assinatura:

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

### Adotando em serviço existente

Instalar autenticação fecha todas as rotas de uma vez. Rode a suíte de testes: as falhas são a sua checklist do que precisa ser marcado público.

Quando marcar tudo num único PR for arriscado demais, `App::allow_unannotated()` desliga o default deny enquanto a migração acontece rota a rota:

```rust
App::new()
    .auth(AuthLayer::<StandardClaims>::new(auth))
    .allow_unannotated()   // temporário — veja o aviso no boot
```

É escape hatch de migração, não configuração. Uma linha visível no builder é melhor do que omissão espalhada por N rotas, e o log de init a denuncia em voz alta a cada boot:

```text
  ⚠️  serverust: default deny DESLIGADO por .allow_unannotated()
     toda rota responde sem identidade. [...]
```

Não afeta `#[authorize]`: afrouxar o default não é abrir mão da permissão que alguém pediu de propósito.

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

`#[authorize]` vem **acima** da macro de rota, como `#[public]`. Abaixo não compila: a rota já teria sido construída, e o escopo exigido não chegaria ao `security` do OpenAPI — um documento que descreve como aberta uma rota fechada é pior do que documento nenhum.

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

## O documento reflete a proteção

Com `App::auth(...)` instalado, o `/openapi.json` passa a descrever a segurança sozinho — e o botão **Authorize** do Scalar e do Swagger UI aparece configurado.

```json
{
  "security": [{ "bearerAuth": [] }],
  "components": {
    "securitySchemes": {
      "bearerAuth": { "type": "http", "scheme": "bearer", "bearerFormat": "JWT" }
    }
  }
}
```

A exigência global espelha o default deny: o padrão do serviço é pedir credencial, e cada operação só aparece no documento quando **diverge** disso.

| Rota | `security` da operação |
|---|---|
| `#[public]` | `[]` — não exige nada, sobrescrevendo o global |
| protegida pelo default deny | `[{"bearerAuth": []}]` |
| `#[authorize(scope = "...")]` | `[{"bearerAuth": ["orders:read"]}]` |

O `[]` da rota pública é um vetor vazio, e não a ausência do campo: ausência herdaria o requisito global e faria o documento afirmar que a rota aberta é protegida.

Sem autenticação instalada, **o documento não fala de segurança**. Declarar um esquema ali descreveria uma exigência que não existe.

Escopos empilhados somam. **Papéis não entram** no documento: a lista do OpenAPI é uma só, e misturar escopo com papel produziria um documento em que ninguém distingue os dois. O guard continua exigindo o papel — o que falta é a descrição, não a verificação.

**Guard escrito à mão não aparece.** `#[guard(MeuGuard)]` verifica o que quiser, mas não tem como declarar o que exige, então a operação fica só com o requisito de identidade. Descrever isso exigiria uma associated const na trait `Guard`, e a troca não pareceu valer: mais superfície pública para todo mundo, por um caso que já é escape hatch.

**O que o portão não alcança:** as rotas de documentação (`/openapi.json`, `/docs`, `/redoc`) não passam por `App::route()` e seguem abertas; feche-as com `App::without_docs()`. E rota registrada direto no `axum::Router`, fora do `App`, também não é embrulhada.

## O que ainda não está implementado

A [ADR 0009](../development/decisions/0009-auth-authz-crate-separada-serverust-auth.md) está entregue. O que ficou de fora dela, e continua fora:

- **Exigência alternativa em `#[authorize]`.** Só há conjunção (AND). Um `any_of` cabe hoje num `#[guard]` escrito à mão.
- **Escopo exigido por `#[guard]` no OpenAPI.** Guard escrito à mão verifica o que quiser, mas não declara o que exige (veja acima).
- **Login, senha e sessão.** Escopo intencional, não lacuna: exige estado e banco, e é território de `torii` e `axum-login`.

## Veja também

- [`serverust-auth/src/jwt.rs`](../../serverust-auth/src/jwt.rs) — verificador e extração do header `Authorization`.
- [`serverust-auth/src/layer.rs`](../../serverust-auth/src/layer.rs) — o layer e o service que ele produz.
- [`serverust-auth/src/claims.rs`](../../serverust-auth/src/claims.rs) — `AuthzFacts`, `Claims` e `StandardClaims`.
- [ADR 0009](../development/decisions/0009-auth-authz-crate-separada-serverust-auth.md) — o desenho completo, incluindo o que está planejado.
