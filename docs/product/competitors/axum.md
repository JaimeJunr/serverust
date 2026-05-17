# Análise Competitiva — Axum

> Última atualização: 2026-05-16
> Versão analisada: axum v0.8.x (2025/2026)
> Fonte: https://github.com/tokio-rs/axum · https://docs.rs/axum

---

## O que é

Axum é o framework web Rust desenvolvido e mantido pelo time do Tokio. Proposta de valor central: **ergonomia máxima no ecossistema Tower/Tokio** — extractors tipados, middleware composável via `tower::Layer`, zero-overhead routing e integração nativa com `hyper`. É o framework que cresce mais rápido em adoção entre os projetos Rust novos (2024-2026).

Por ser mantido pelo mesmo time que mantém Tokio, Axum tem garantia de compatibilidade de longo prazo com o runtime assíncrono mais usado em Rust.

---

## O que tem de bom

| Aspecto | Avaliação |
|---|---|
| Ergonomia de extractors | Excelente — `Path<T>`, `Query<T>`, `Json<T>`, `State<S>` composáveis |
| Middleware | Tower ecosystem completo — `tower-http`, `tower::ServiceBuilder` |
| Comunidade | Grande e crescente — segundo framework mais popular no ecossistema Rust |
| Documentação | Muito boa, com examples abrangentes no repositório oficial |
| Async nativo | Sim, Tokio + hyper |
| Performance | Muito alta — no mesmo patamar de actix-web em benchmarks HTTP/1.1 e HTTP/2 |
| Integração | Funciona nativamente com qualquer crate do ecossistema Tower |

---

## O que falta vs serverust

### Dependency Injection

Axum não tem DI nativo. O padrão é `State<Arc<AppState>>` — um struct com todos os recursos compartilhados. Funciona para projetos pequenos, mas escala mal: qualquer adição ao estado requer mudança na assinatura de todos os handlers que precisam de `State`.

serverust tem `#[injectable]` + Container com resolução automática por tipo — handlers declaram apenas as dependências que realmente precisam.

### AWS Lambda

Axum pode rodar em Lambda via `lambda_http` (crate oficial da AWS). A integração existe, mas não é transparente:

- O usuário precisa chamar `lambda_http::run(app)` explicitamente.
- Sem detecção automática de runtime (Lambda vs long-running).
- Cold start: sem otimizações específicas para Lambda — o binário inclui todo o hyper stack, tipicamente resultando em >100ms no ARM64 128MB.

serverust detecta `AWS_LAMBDA_FUNCTION_NAME` automaticamente e usa `serverust-lambda` sem config extra. O cold start é otimizado desde o design: binário stripped < 10MB, p95 < 50ms no ARM64 128MB.

### OpenAPI

Axum não gera OpenAPI automaticamente. As opções da comunidade são:

- **utoipa**: popular, mas requer anotações manuais em cada handler (`#[utoipa::path(...)]`).
- **aide**: integração mais próxima do Axum, mas ainda opt-in e com configuração extra.

serverust gera OpenAPI 3.1 automaticamente a partir das macros `#[get]`, `#[post]`, etc., sem anotações extras. Scalar UI embutido disponível em `/docs`.

### Kafka / Event Sources

Axum é HTTP-only por design. Para Kafka em Lambda, o desenvolvedor precisa de uma stack paralela (lambda_runtime + rdkafka crus), sem compartilhamento de DI, middleware ou handlers com o servidor HTTP.

serverust resolve com `serverust-events` opt-in: mesmo DI container, mesmas macros, mesmo projeto — cobrindo HTTP e Kafka/SQS.

### CLI

Axum não tem CLI de scaffolding. Iniciar um projeto requer boilerplate manual ou templates comunitários de terceiros.

serverust tem `serverust-cli` com `new`, `generate`, `dev`, `build`, `deploy` e `openapi`.

---

## Comparação lado a lado

| Feature | serverust | axum |
|---|:---:|:---:|
| AWS Lambda nativo | ✅ | ⚠️ (via lambda_http, manual) |
| Runtime dual HTTP ↔ Lambda | ✅ | ❌ |
| Kafka event source nativo | ✅ | ❌ |
| SQS / EventBridge / S3 event source | ✅ | ❌ |
| OpenAPI 3.1 automático | ✅ | ❌ (utoipa manual) |
| Scalar / docs UI embutido | ✅ | ❌ |
| Dependency Injection nativo | ✅ | ❌ (State<Arc<T>> manual) |
| CLI scaffolding | ✅ | ❌ |
| Binário stripped < 10 MB | ✅ | ⚠️ (depende do projeto) |
| Cold start < 50 ms (Lambda ARM64) | ✅ | ❌ (tipicamente >100ms) |
| Middleware Tower | ⚠️ (API própria) | ✅ (ecosystem completo) |
| WebSockets | ✅ (via axum-ws) | ✅ nativo |
| Comunidade / ecossistema | menor | grande |

---

## Por que serverust sobre axum

**Se o deployment é AWS Lambda**: serverust foi projetado para Lambda desde o primeiro commit. Axum pode rodar em Lambda, mas sem otimizações de cold start, sem detecção automática de runtime e sem event sources nativos — o resultado é mais boilerplate e cold starts mais altos.

**Se o projeto usa Kafka/SQS junto com HTTP**: serverust unifica os dois em um projeto, com o mesmo DI e mesmas macros. Com Axum, o desenvolvedor mantém dois projetos separados com stacks diferentes.

**Se OpenAPI é requisito**: serverust gera o schema automaticamente; com Axum, isso exige utoipa ou aide com anotações manuais em cada endpoint.

**Se DI escalável importa**: `#[injectable]` do serverust escala melhor que `State<Arc<AppState>>` em projetos com muitas dependências.

**Quando Axum pode ser melhor**: projetos que já usam o ecossistema Tower extensivamente (middleware customizado, integrações via `tower::Service`), projetos que não usam Lambda e não precisam de Kafka, ou projetos que preferem uma comunidade maior com mais exemplos e templates.

---

## Quando faz sentido usar axum vs serverust

| Cenário | Recomendação |
|---|---|
| API REST em servidor long-running, sem Lambda | **axum** |
| Integração profunda com Tower middleware | **axum** |
| AWS Lambda (HTTP e/ou Kafka/SQS) | **serverust** |
| HTTP + Kafka no mesmo projeto | **serverust** |
| OpenAPI automático sem config manual | **serverust** |
| DI escalável em projeto grande | **serverust** |
| Cold start crítico em Lambda | **serverust** |
