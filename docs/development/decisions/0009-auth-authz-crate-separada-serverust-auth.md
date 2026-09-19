# ADR 0009 — Auth/Authz em crate separada serverust-auth: JWT de IdP externo + RBAC em compile-time

- **Status:** Proposed
- **Date:** 2026-09-19
- **Deciders:** maintainers serverust

---

## Contexto e Problema

O serverust não oferece autenticação nem autorização. Hoje o usuário que precisa proteger uma rota tem que montar tudo à mão: validar o token, propagar a identidade até o handler e checar permissão. É exatamente o boilerplate que a [filosofia do projeto](../../product/philosophy.md) classifica como bug de design.

O ecossistema Rust não oferece um caminho pronto. O levantamento feito antes desta ADR mostrou que as opções "framework completo" são imaturas (`better-auth-rs` em v0.1.0 com 29 downloads; `zanzibar`/`keldra` publicados há um dia) ou descontinuadas (a biblioteca open-source do `oso` foi deprecada em 2023-12-18 em favor do Oso Cloud, e o crate Rust está parado desde 2024-01). O que é maduro são os blocos fundamentais: `jsonwebtoken` (191M downloads), `argon2` (54M), `oauth2` (51M), `openidconnect` (13.5M), `biscuit-auth` (11.5M), `casbin` (3.1M).

Ou seja: em Rust a prática madura é montar authn/authz a partir de micro-crates. **Esse é precisamente o gap de DX que o serverust existe para fechar** — sem abrir mão das garantias de compile-time e sem regredir os invariantes de cold start e tamanho de binário.

Há ainda um problema que nenhuma análise genérica de Rust cobre, porque só aparece em Lambda: a validação de JWT contra um IdP OIDC exige buscar um JWKS por HTTPS. Se essa busca acontecer na primeira invocação, ela paga handshake TLS e resolução de DNS na fase de invocação — onde **não há o burst de CPU** da fase de init. É o mesmo padrão que custou 735 ms no caso de produção documentado na issue #40.

## Drivers de Decisão

- **Invariantes públicos**: cold start ARM64 < 50 ms p95 e binário `hello-world` stripped < 10 MB não podem regredir. Cripto é pesada; usuários sem auth não podem pagar por ela.
- **`serverust-core` é dep base universal** — qualquer adição tem custo para todo mundo (ADR 0003).
- **"Segurança vem da linguagem, não da disciplina do time"** — esquecer de proteger uma rota deve ser difícil, idealmente impossível.
- **"Compile-time sobre runtime"** — autorização não deve custar lookup, alocação ou I/O por request.
- **Cross-compilação x86_64 → aarch64 precisa continuar funcionando** sem exigir cmake nem toolchain C (issue #40, item 4).
- **Lambda é stateless e efêmero** — sessão em banco é mau encaixe; verificação de token sem estado é o encaixe natural.

## Opções Consideradas

### Onde o código mora

1. **Crate nova `serverust-auth`** (escolhida) — segue o precedente da ADR 0003.
2. `serverust-core` atrás de feature flag — rejeitada: viola o princípio de core limpo e arrasta cripto para a árvore de deps de todo projeto.
3. `serverust-lambda` — rejeitada: auth é conceito HTTP, não de adapter de runtime.

### Escopo da autenticação

1. **Validar token emitido por IdP externo** (escolhida) — Cognito, Auth0, Clerk, Keycloak, Logto.
2. Auth completo self-hosted (login, senha, sessão) — rejeitada para v1: superfície de segurança muito maior, exige banco e estado, e sessão em banco é mau encaixe para Lambda. É o território do `torii`/`axum-login`.
3. Só traits genéricas, sem opinar em JWT — rejeitada: transfere de volta ao usuário o boilerplate que motivou a ADR.

### Modelo de autorização

1. **RBAC/scopes resolvidos em compile-time** (escolhida).
2. RBAC + ganchos ABAC assíncronos — adiado: abre I/O no caminho de autorização; reavaliar quando houver demanda real.
3. Motor de policy (`casbin`) — adiado: carrega config em runtime, contraria "compile-time sobre runtime".
4. Biscuit (authz dentro do token) — adiado: tecnicamente excelente para Lambda (verificação offline, atenuação de permissões), mas paradigma pouco familiar. Candidato a feature opt-in futura.

## Decisão

Criar `serverust-auth` como crate independente, com quatro decisões estruturais:

### 1. Authn é um `Layer`; authz é um `Guard` que lê o resultado

O trait `Guard` atual tem assinatura estática — `check(parts: &Parts)`, sem `&self` — e o `GuardCheck` **descarta o state** do axum (`pipeline.rs:54`). Isso parecia impedir um guard de JWT, que precisa da chave de verificação.

A saída é não precisar de state no guard:

```
App::auth(...) instala um Layer que detém o verificador
        ↓
Layer valida o token UMA VEZ e insere as claims em parts.extensions
        ↓
   ├── Auth<C> (extractor)  → lê Arc<C> das extensions, tipado, sem revalidar
   └── Guard de authz       → lê Arc<dyn AuthzFacts> das extensions
```

O layer insere dois ponteiros para a **mesma alocação** (coerção `Arc<C>` → `Arc<dyn AuthzFacts>` é gratuita): um tipado para o extractor, um apagado para os guards genéricos, que não conhecem `C`.

**Consequência importante: o trait `Guard` não muda de assinatura.** Nenhuma quebra de API, e o mecanismo existente de `#[guard]` é reaproveitado.

### 2. O JWKS é aquecido na fase de init — por construção da API

O construtor OIDC é `async` e **deve ser aguardado no `main()`, antes do `run()`**:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Busca o JWKS aqui: fase de init, com burst de CPU.
    let auth = JwtAuth::oidc(ISSUER).await?;

    App::new().auth(auth).route(create_order).run().await
}
```

A alternativa preguiçosa (buscar o JWKS no primeiro request) seria mais simples de implementar e **mais cara em produção**, pelo motivo documentado na issue #40. A assinatura da API move o custo para onde ele é barato, sem exigir que o usuário conheça a pegadinha.

Rotação de chave é tratada por cache com TTL e refresh sob demanda em caso de `kid` desconhecido. Não há tarefa de background: entre invocações o Lambda é congelado, então timer de refresh não é confiável.

Para segredo simétrico ou chave estática não há rede, e os construtores são síncronos (`JwtAuth::hs256`, `JwtAuth::rs256_pem`).

### 3. Autorização é compile-time e alimenta o OpenAPI

Const generics de `&'static str` não são estáveis em Rust, então `RequireScope<"orders:write">` não compila. A macro `#[authorize]` gera um tipo unitário que implementa `Guard`, e injeta o `GuardCheck` correspondente:

```rust
#[authorize(scope = "orders:write")]
#[post("/orders")]
async fn create_order(user: Auth<MyClaims>, Json(dto): Json<CreateOrder>) -> ... { }
```

Zero lookup, zero alocação, zero rede no caminho de autorização — só comparação de strings estáticas contra as claims já extraídas.

Para o OpenAPI, adiciona-se ao trait `Guard` uma associated const **com default**:

```rust
pub trait Guard: Send + Sync + 'static {
    fn check(parts: &Parts) -> impl Future<Output = Result<(), Response>> + Send;

    /// Escopos exigidos, para o `security` do OpenAPI. Default vazio.
    const REQUIRED_SCOPES: &'static [&'static str] = &[];
}
```

Por ter default, é **mudança não-quebrante**: implementações existentes seguem compilando. A macro de rota, que já monta um `OperationBuilder` (`serverust-macros/src/lib.rs:198`), passa a emitir `.security(...)` lendo `<G as Guard>::REQUIRED_SCOPES` dos parâmetros `GuardCheck<G>`. O `App::auth()` registra o `securityScheme` nos `Components` (`serverust-core/src/openapi.rs:75`).

Resultado: **a rota se documenta sozinha** e o botão "Authorize" do Scalar/Swagger UI passa a funcionar sem configuração. É a única adição em `serverust-core`, e não traz dependência nova.

### 4. Backend de cripto em Rust puro por default

`jsonwebtoken` v11 tem backend plugável: `aws-lc-rs` é opcional, e há implementações Rust puras (`rsa`, `p256`, `p384`, `ed25519-dalek`, `hmac`, `sha2`). O default de `serverust-auth` **não** usa `aws-lc-rs`, que exige cmake e toolchain C e quebra a cross-compilação x86_64 → aarch64 (issue #40, item 4). Quem quiser o backend acelerado o habilita por feature.

## Ponto em aberto (decidir antes de implementar)

**Rota sem `#[authorize]`: pública ou negada?**

- **Default deny** — toda rota exige token válido, salvo marcação explícita `#[public]`. Alinhado a "segurança vem da linguagem": esquecer a anotação falha fechado. Custo: mais atrito, e quebra o `App` de quem adicionar `.auth()` a um serviço existente.
- **Default allow** — só rotas anotadas são protegidas. Menos atrito, mas esquecer a anotação deixa o endpoint aberto — exatamente a classe de erro que a filosofia diz que não deve depender de disciplina.

Recomendação: **default deny**, com `#[public]` explícito. Observar que as rotas de documentação (`/openapi.json`, `/docs`, `/redoc`) ficam fora dos layers por design (`App::interceptor`), então continuam públicas — quem precisa fechá-las usa `App::without_docs()`.

## Consequências

### Positivas

- `serverust-core` continua sem dependência de cripto; quem não usa auth não paga nada em binário nem em cold start.
- O caminho rápido vira o caminho default: o JWKS é aquecido no init por construção da API, não por disciplina do usuário.
- Autorização custa comparação de `&'static str` — sem I/O, sem alocação, sem estado.
- OpenAPI com `security` correto sem trabalho manual.
- `Guard` não muda de assinatura; `#[guard]` segue funcionando.

### Negativas / Trade-offs

- Mais um crate para manter e publicar.
- Não cobre login, senha nem sessão — quem precisa disso usa `torii`/`axum-login` ou um IdaaS. É escopo intencional, não lacuna acidental.
- RBAC por scopes não expressa regra dependente de recurso ("só o dono edita"); isso fica para os ganchos ABAC de uma futura ADR.
- Uma associated const nova em `Guard` no core — mitigada pelo default, mas é superfície pública adicional.

## Verificação

```bash
# serverust-core deve continuar sem cripto e sem auth
cargo tree -p serverust-core | grep -E "jsonwebtoken|rsa|p256|aws-lc-rs|ring"

# hello-world não pode ganhar peso: invariante de 10 MB
scripts/benchmark_ci.sh

# cross-compilação ARM64 sem toolchain C
cargo lambda build --release --arm64 -p <exemplo-auth>
```

## Links e Referências

- [ADR 0003](0003-event-driven-crate-separada-serverust-events.md) — precedente de crate separada para feature pesada
- [ADR 0002](0002-dynamodb-feature-opt-in-serverust-telemetry.md) — precedente de feature opt-in
- [`docs/product/philosophy.md`](../../product/philosophy.md) — "segurança vem da linguagem", "compile-time sobre runtime"
- [`CLAUDE.md`](../../../CLAUDE.md) — invariantes e regra de feature pesada opt-in
- Issue #40 — custo de TLS/credencial fora do `Init Duration`; escolha de TLS na cross-compilação
