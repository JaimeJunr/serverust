# ADR 0009 — Auth/Authz em crate separada serverust-auth: JWT de IdP externo + RBAC em compile-time

- **Status:** Accepted
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

### 5. Default deny: rota sem anotação é negada

Com `App::auth(...)` instalado, **toda rota exige token válido**, salvo marcação explícita `#[public]`. A alternativa — proteger só o que está anotado — foi rejeitada.

Três razões, em ordem de peso:

**Falha por omissão é invisível sob default allow.** Um endpoint novo sem anotação compila, passa nos testes e produz um diff idêntico ao das outras rotas. Nada no sistema reclama. Sob default deny, a mesma omissão devolve 401 na primeira chamada: **um buraco de segurança silencioso vira um bug funcional barulhento**, e bug funcional é sempre corrigido porque bloqueia o caminho feliz.

**Presença é auditável; ausência não é.** Sob default deny, `grep -r '#\[public\]'` devolve a superfície aberta inteira e completa. Sob default allow, descobrir o que está desprotegido exige enumerar todas as rotas e verificar a *ausência* de anotação em cada uma — e não existe grep para ausência. A diferença é decisiva quando a revisão é feita por trecho, sem o roteador inteiro em contexto.

**É o idioma do Rust.** A linguagem é fechada por padrão em tudo: item é privado até se escrever `pub`, binding é imutável até se escrever `mut`, `unsafe` exige opt-in. `#[public]` é o `pub` das rotas. O mesmo padrão é o recomendado pela documentação de autenticação do NestJS (guard global via `APP_GUARD` + decorator `@Public()`), referência arquitetural declarada do projeto. E é o princípio de *fail-safe defaults* de Saltzer & Schroeder (1975).

O trade-off é real e assumido: adicionar `.auth()` a um serviço existente fecha todas as rotas de uma vez. Mitigações que fazem parte da decisão:

1. **401 que ensina** — o corpo da rejeição por falta de anotação diz o que fazer (`adicione #[authorize(...)] ou #[public]`), em vez de um 401 mudo.
2. **Log de init com as rotas públicas** — `App::auth()` registra a lista de rotas `#[public]` na inicialização. Em Lambda isso aparece no log de init de todo deploy, mantendo a superfície aberta visível sem auditoria ativa.
3. **Escape hatch global e explícito** — `.allow_unannotated()` para migração de serviço existente: uma linha visível e auditável em vez de omissão distribuída por N rotas, nomeada para desencorajar permanência.

Nota: as rotas de documentação (`/openapi.json`, `/docs`, `/redoc`) ficam fora dos layers por design (`App::interceptor`), então seguem públicas mesmo sob default deny — quem precisa fechá-las usa `App::without_docs()`.

## Consequências

### Positivas

- `serverust-core` continua sem dependência de cripto; quem não usa auth não paga nada em binário nem em cold start.
- O caminho rápido vira o caminho default: o JWKS é aquecido no init por construção da API, não por disciplina do usuário.
- Autorização custa comparação de `&'static str` — sem I/O, sem alocação, sem estado.
- OpenAPI com `security` correto sem trabalho manual.
- `Guard` não muda de assinatura; `#[guard]` segue funcionando.
- Esquecer de proteger uma rota falha fechado e em voz alta, em vez de publicar um endpoint aberto em silêncio.
- A superfície pública fica auditável por presença (`grep '#\[public\]'`) e visível no log de init de cada deploy.

### Negativas / Trade-offs

- Mais um crate para manter e publicar.
- Não cobre login, senha nem sessão — quem precisa disso usa `torii`/`axum-login` ou um IdaaS. É escopo intencional, não lacuna acidental.
- RBAC por scopes não expressa regra dependente de recurso ("só o dono edita"); isso fica para os ganchos ABAC de uma futura ADR.
- Uma associated const nova em `Guard` no core — mitigada pelo default, mas é superfície pública adicional.
- Adotar `.auth()` em um serviço existente fecha todas as rotas de uma vez, exigindo uma passada para marcar as públicas. É atrito único e guiado pela própria suíte de testes, mas é atrito real.
- O default deny é imposto em runtime, não em compile-time: a macro de rota não sabe se `App::auth()` foi chamado. É fail-safe, não garantia de compilador — a ambição de transformar isso em erro de compilação fica para uma ADR futura.

## Verificação

```bash
# serverust-core deve continuar sem cripto e sem auth.
# Compara NOME DE PACOTE exato: um `grep -E "...|ring"` solto casa dentro de
# `inlinable_string` e acusa regressão que não existe.
cargo tree -p serverust-core --prefix none | awk '{print $1}' | sort -u \
  | grep -xE "jsonwebtoken|rsa|p256|p384|ed25519-dalek|aws-lc-rs|ring|sha2|hmac" \
  && echo "REGRESSÃO: cripto entrou no core" || echo "ok: core sem cripto"

# nenhum membro do workspace deve depender de serverust-auth por acidente
cargo tree -i serverust-auth --workspace

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
