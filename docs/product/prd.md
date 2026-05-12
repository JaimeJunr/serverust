# PRD — RustAPI Framework

## 1. Introduction / Overview

**RustAPI** é um framework Rust opinativo para construir APIs HTTP e funções AWS Lambda, projetado para unir três influências:

- **Rust + Axum + Tokio + Tower** como motor (performance nativa, cold start baixo, segurança em compile-time).
- **FastAPI** como filosofia de DX (rotas declarativas, validação automática, OpenAPI/Swagger gerado a partir dos tipos, pouca cerimônia).
- **NestJS** como arquitetura (Modules, Dependency Injection, Controllers/Services, Guards, Pipes, Interceptors, CLI com `new` / `generate` / `dev` / `build` / `deploy`).

O objetivo é entregar a produtividade e a organização que faltam ao Axum puro, sem abrir mão da performance que falta ao FastAPI, com **suporte nativo a AWS Lambda** desde o MVP. A mesma aplicação roda em Lambda (via `lambda_http`) ou em servidor HTTP local, sem alteração de código de negócio.

A frase-síntese: **"a experiência do FastAPI + a arquitetura do NestJS, rodando na performance e segurança do Rust"**.

## 2. Goals

1. Permitir criar uma API CRUD funcional em AWS Lambda com **OpenAPI automático em menos de 50 linhas** de código de usuário.
2. Cold start em Lambda **< 50ms** (ARM64, 128MB) para hello-world; binário stripado **< 10MB**.
3. Validação de entrada, serialização de saída, tratamento de erros e geração de OpenAPI **sem código boilerplate** — tudo derivado de tipos Rust.
4. Arquitetura modular NestJS-like (Modules + DI + Controllers + Services + Guards + Pipes + Interceptors) **idiomática em Rust**, sem reflection runtime.
5. CLI com paridade conceitual ao `@nestjs/cli`: `serverust new`, `serverust generate <resource|module|controller|service|pipe|guard|interceptor|filter>`, `serverust dev`, `serverust build`, `serverust deploy lambda`, `serverust info`.
6. Observabilidade serverless integrada (logger estruturado, tracing, métricas, idempotência) — reimplementando os conceitos do AWS Lambda Powertools nativamente em Rust.
7. Detecção automática de ambiente (Lambda vs HTTP local) com um único `App::new().run().await`.

## 3. User Stories

### US-1 — Bootstrap de projeto via CLI
**Como** desenvolvedor backend, **quero** criar um novo projeto RustAPI com um único comando, **para que** eu comece a codar regras de negócio em minutos.

Acceptance criteria:
- [ ] `serverust new funds-api` cria estrutura completa (`src/`, `modules/`, `shared/`, `Cargo.toml`, `serverust.toml`).
- [ ] Projeto recém-criado compila com `cargo build` sem ajustes.
- [ ] `serverust dev` sobe servidor HTTP local com hot-reload e `/docs` acessível.
- [ ] `serverust info` imprime versões de runtime, toolchain e features ativas.

### US-2 — Definir rota com macro declarativa
**Como** desenvolvedor, **quero** declarar um endpoint usando atributos `#[get]`, `#[post]`, **para que** eu obtenha roteamento, extração e documentação OpenAPI sem boilerplate.

Acceptance criteria:
- [ ] Função anotada com `#[post("/users")]` é registrada automaticamente no roteador.
- [ ] Parâmetros `Json<T>`, `Path<T>`, `Query<T>`, `State<T>` são extraídos de forma tipada.
- [ ] Schema OpenAPI gerado a partir dos tipos aparece em `/openapi.json` e `/docs`.

### US-3 — Validação automática com erros padronizados
**Como** desenvolvedor, **quero** que o framework valide o payload antes de chamar meu handler, **para que** entradas inválidas retornem JSON de erro consistente sem código adicional.

Acceptance criteria:
- [ ] `#[derive(Validate)]` em DTO aplica regras (`length`, `range`, `email`, `custom`) automaticamente.
- [ ] Falha de validação retorna HTTP 422 com payload `{ "error": "validation_error", "fields": { ... } }`.
- [ ] Erros de domínio derivados de `#[derive(ApiError)]` mapeiam `#[status(404)]`, `#[message("...")]` para resposta HTTP padronizada.

### US-4 — Modules + Dependency Injection (NestJS-like)
**Como** arquiteto, **quero** declarar módulos com providers e injetar dependências em controllers/services, **para que** projetos grandes mantenham coesão e testabilidade.

Acceptance criteria:
- [ ] Macro `#[module]` declara providers, controllers e imports.
- [ ] Macro `#[injectable]` registra um service no container DI.
- [ ] Controllers recebem services via parâmetro tipado, resolvidos em compile-time (sem reflection).
- [ ] Testes unitários podem substituir um provider por um mock via API do container.

### US-5 — Guards, Pipes e Interceptors
**Como** desenvolvedor, **quero** declarar Guards (autorização), Pipes (transformação/validação) e Interceptors (cross-cutting) por rota ou por módulo, **para que** eu separe responsabilidades de forma idiomática.

Acceptance criteria:
- [ ] `#[guard(AuthGuard)]` aplica verificação antes do handler.
- [ ] Pipes transformam input antes de chegar ao handler (ex: `ParseUuidPipe`).
- [ ] Interceptors envolvem a execução (ex: timing, transformação de resposta).
- [ ] Falha em Guard retorna HTTP 401/403 sem chamar o handler.

### US-6 — Mesma app em Lambda e HTTP local
**Como** dev, **quero** rodar o mesmo binário em AWS Lambda e localmente, **para que** o ciclo de desenvolvimento seja rápido e o deploy seja trivial.

Acceptance criteria:
- [ ] `App::new().routes(...).run().await` detecta ambiente (`AWS_LAMBDA_RUNTIME_API`) e escolhe entre `lambda_http` e servidor HTTP local.
- [ ] `serverust deploy lambda` empacota binário ARM64 e publica via SAM/Cargo Lambda.
- [ ] Cold start medido < 50ms para hello-world em 128MB ARM64.

### US-7 — Observabilidade integrada (Powertools nativo em Rust)
**Como** SRE, **quero** logs estruturados, tracing distribuído, métricas EMF e idempotência fora da caixa, **para que** Lambdas em produção sejam observáveis sem código extra.

Acceptance criteria:
- [ ] Logger estruturado JSON com correlation id (X-Ray trace id) habilitado por padrão.
- [ ] Tracing via `tracing` + OpenTelemetry compatível com AWS X-Ray.
- [ ] Métricas EMF (Embedded Metric Format) emitidas via macro `#[metric]`.
- [ ] Decorator `#[idempotent(key = "...")]` persiste resposta em DynamoDB e deduplica.

### US-8 — CLI generate completo (NestJS-like)
**Como** dev, **quero** comandos `serverust generate` para scaffolding, **para que** padrões arquiteturais sejam consistentes em todo o projeto.

Acceptance criteria:
- [ ] `serverust g resource <name>` cria `routes.rs`, `handlers.rs`, `schemas.rs`, `service.rs`, `repository.rs`, `errors.rs`, registra no módulo pai.
- [ ] `serverust g module|controller|service|pipe|guard|interceptor|filter <name>` cria arquivos correspondentes.
- [ ] Arquivos gerados compilam imediatamente.

## 4. Functional Requirements

### FR-1 — Roteamento declarativo
1. Macros `#[get]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]` com paths estilo `/users/{id}`.
2. Extractors built-in: `Path<T>`, `Query<T>`, `Json<T>`, `State<T>`, `Headers`, `Cookies`, `Multipart`.
3. Roteador construído em compile-time a partir das anotações.

### FR-2 — Validação
1. `#[derive(Validate)]` integrando `validator` ou `garde`.
2. Resposta HTTP 422 padronizada com lista de campos inválidos.
3. Validators custom via `#[validate(custom = "fn_name")]`.

### FR-3 — OpenAPI / Swagger
1. Geração automática de `/openapi.json` (OpenAPI 3.1) a partir dos tipos via `utoipa` ou `aide`.
2. Swagger UI servida em `/docs`, ReDoc em `/redoc`.
3. Customização via `App::new().openapi_info(...).docs("/docs")`.

### FR-4 — Erros padronizados
1. `#[derive(ApiError)]` com `#[status(...)]` e `#[message(...)]`.
2. Conversão automática `?` em handlers `Result<T, E: ApiError>`.
3. Payload JSON consistente: `{ "error": "code", "message": "...", "details": { ... } }`.

### FR-5 — Modules & DI
1. `#[module]` declara `providers`, `controllers`, `imports`, `exports`.
2. `#[injectable]` registra service; lifetimes: `Singleton` (default), `Scoped` (por request), `Transient`.
3. Resolução em compile-time via macros + traits; sem reflection.
4. API de teste para substituir providers por mocks.

### FR-6 — Guards / Pipes / Interceptors
1. `#[guard(...)]`, `#[pipe(...)]`, `#[interceptor(...)]` em handler, controller ou módulo.
2. Trait `Guard`, `Pipe<T>`, `Interceptor` definidos pelo framework.
3. Ordem de execução: Guards → Pipes → Handler → Interceptors.

### FR-7 — Runtime dual (Lambda + HTTP)
1. `App::new().run().await` detecta `AWS_LAMBDA_RUNTIME_API`.
2. APIs explícitas: `run_http(addr)`, `run_lambda()`.
3. Compatível com API Gateway REST, HTTP API e Lambda Function URLs.

### FR-8 — Observabilidade (Powertools nativo)
1. Logger estruturado JSON com `tracing` + `tracing-subscriber`.
2. Correlation ID extraído de `X-Amzn-Trace-Id` ou gerado.
3. Métricas EMF emitidas via macro `#[metric(name, unit)]`.
4. Idempotência via `#[idempotent]` com storage DynamoDB (default) ou custom.
5. Integração `aws-sdk-rust` para X-Ray, CloudWatch.

### FR-9 — CLI
1. `serverust new <name> [--strict]`.
2. `serverust generate (g) <kind> <name>` para `resource`, `module`, `controller`, `service`, `pipe`, `guard`, `interceptor`, `filter`.
3. `serverust dev` (watch mode, hot reload via `cargo-watch`).
4. `serverust build [--release]`.
5. `serverust deploy lambda [--arch arm64|x86_64]` (usa `cargo lambda`).
6. `serverust info`.
7. `serverust openapi --out openapi.json` (exporta spec sem subir servidor).

### FR-10 — Configuração
1. Arquivo `serverust.toml` na raiz com seções `[server]`, `[lambda]`, `[telemetry]`, `[openapi]`.
2. Override por variáveis de ambiente (`figment` ou `config`).
3. Profiles por ambiente (`dev`, `staging`, `prod`).

### FR-11 — Estrutura de projeto padrão
```
src/
  main.rs
  app.rs
  config.rs
  modules/
    <feature>/
      mod.rs            # #[module]
      controller.rs
      service.rs
      schemas.rs
      repository.rs
      errors.rs
  shared/
    database.rs
    middlewares.rs
    telemetry.rs
serverust.toml
Cargo.toml
```

## 5. Non-Goals (out of scope no MVP)

- Frontend / SSR / templating engine.
- ORM proprietário (RustAPI **não** vai competir com `sqlx`, `sea-orm`, `diesel` — apenas integrar).
- WebSockets e Server-Sent Events (fase 2).
- gRPC nativo (fase 2 via tonic adapter).
- Suporte a outros providers serverless (GCP Cloud Run, Azure Functions) — fase 2.
- Reflection runtime para DI (Rust não suporta idiomaticamente — usaremos macros + builders).
- Compatibilidade binária com FastAPI ou NestJS (a inspiração é de filosofia/DX, não de wire format).
- Hot module reload em runtime (apenas `cargo-watch` no `serverust dev`).
- **Macro `#[idempotent]` e schema DynamoDB de idempotência** (trait `IdempotencyStore` é exposta no MVP, mas a integração ergonômica fica para fase 2).
- **`bacon` / subsecond rebuilds** (fase 2; MVP usa `cargo-watch`).
- **Macros para DI/módulos** estilo `@Module()` do NestJS: DI permanece em builders explícitos no MVP. Reavaliar se a comunidade demandar.
- Suporte a Node wrapper na CLI (CLI é 100% Rust).

## 6. Design Considerations

- **Sintaxe**: priorizar attribute macros (`#[get]`, `#[post]`) e `#[derive(...)]` para reduzir boilerplate em rotas e DTOs. **DI e composição de `App` ficam explícitas via builder** — evitar "magic" demais (preserva mensagens de erro claras e tempos de compilação aceitáveis).
- **Compile-time over runtime**: introspecção de rotas, schema OpenAPI e tabelas de providers são resolvidas em compile-time. Zero overhead runtime, tipagem forte.
- **Errors**: padronizar payload de erro JSON para todo o framework, inspirado em Problem Details (RFC 7807) mas pragmático.
- **Layering sobre Axum**: RustAPI **estende** Axum, não o esconde. Escape hatch público `App::axum_router()` retorna o `axum::Router` interno para usuários avançados que precisem de funcionalidades não cobertas pelo framework.
- **Codename "RustAPI"**: usado internamente; nome público definido próximo do release (após verificação no crates.io e branding).

## 7. Technical Considerations

### Toolchain e edition
- **MSRV: Rust 1.85+ (Edition 2024)**. Justificativas:
  - Edition 2024 traz refinamentos no sistema de tipos e nas macros relevantes para frameworks opinativos.
  - `aws-lambda-rust-runtime` exige MSRV 1.84+ desde janeiro/2026 — abaixo disso, o adapter Lambda nem compila.
  - Permite uso nativo de `async fn` em traits e async closures (`async || {}`), simplificando macros e middlewares.
  - Habilita compatibilidade futura com **AWS Lambda Managed Instances** (GA 2026-03), que dependem das melhorias de concorrência das versões recentes do SDK.
- **Cargo workspace**: `serverust-core`, `serverust-macros`, `serverust-cli`, `serverust-lambda`, `serverust-telemetry`.

### Stack interna (decisões fechadas)
- **HTTP/runtime**: Tokio · Axum · Tower / tower-http.
- **Serialização**: Serde.
- **Validação**: `validator` (com `garde` em avaliação contínua para fase 2).
- **OpenAPI**: **utoipa** (escolha definida — maturidade, derive macros estáveis, integração nativa `utoipa-axum`).
- **Tracing/logs**: `tracing` + `tracing-subscriber` + OpenTelemetry.
- **Lambda**: `lambda_http`, `aws_lambda_events`.
- **AWS SDK**: `aws-sdk-rust` (DynamoDB para idempotência, CloudWatch EMF).
- **Config**: `figment`.
- **Erros**: `thiserror`.

### Padrões arquiteturais (decisões fechadas)
- **Macros**: crate separado `serverust-macros` (`proc-macro = true`). Macros declarativas cobrem rotas (`#[get]`, `#[post]`, ...) e DTOs (`#[derive(Validate)]`, `#[derive(ApiError)]`). DI e composição de `App` permanecem em builders explícitos.
- **DI híbrido**: `Arc<dyn Trait>` por default (cobre 95% dos casos com DX ergonômica); opt-in para generics/static dispatch quando o usuário marcar provider com `#[injectable(static)]` (performance crítica). Tabela de providers gerada em compile-time, sem reflection.
- **CLI**: 100% Rust com `clap`. Distribuído via `cargo install serverust-cli`. Sem dependência de Node.
- **Hot reload em `serverust dev`**: `cargo-watch` no MVP (`bacon` fica como tarefa de roadmap pós-MVP).
- **Idempotência**: trait `IdempotencyStore` com implementação default em **DynamoDB**. Usuários podem fornecer impls custom (Redis, Postgres, in-memory para testes) sem fork do framework.
- **Observabilidade no MVP**: logger JSON estruturado + tracing/OTel + métricas EMF. **Idempotência completa fica para fase 2** (a trait `IdempotencyStore` é exposta no MVP, mas a macro `#[idempotent]` e o schema DynamoDB são entregues na próxima fase).
- **Escape hatch**: `App::axum_router() -> &mut axum::Router` exposto e documentado como API pública estável.

### Compatibilidade
- **Lambda**: testar com API Gateway REST v1, HTTP v2 e Function URLs (via `lambda_http`).
- **Lambda Managed Instances**: arquitetura preparada para multi-tenancy/multi-threading real dentro de um único execution environment (sem `static mut` em estado de aplicação; usar `Arc` desde o início).

### Riscos técnicos
- Complexidade das macros pode degradar tempos de compilação — monitorar com `cargo build --timings` e otimizar.
- `utoipa` precisa cobrir extractors customizados — se algum gap aparecer, contribuir upstream em vez de fork.
- `Arc<dyn Trait>` default pode mascarar problemas de performance — incluir benchmark contínuo no CI.

## 8. Architecture & Diagrams

- [Architecture](diagrams/architecture.excalidraw) — componentes do framework (core, macros, CLI, lambda adapter, telemetry) e integrações externas (AWS Lambda, API Gateway, CloudWatch, X-Ray, DynamoDB).
- [Sequence](diagrams/sequence.excalidraw) — fluxo temporal de uma requisição: API Gateway → lambda_http → Router → Guards → Pipes → Handler → Interceptors → Response.
- [Data Flow](diagrams/data-flow.excalidraw) — transformação de dados: JSON cru → deserialização → validação → DTO tipado → service → response → serialização → JSON.

> Nota: os diagramas co-evoluem com este PRD. Tanto humanos (excalidraw.com / desktop) quanto as skills `prd`/`excalidraw-diagram` podem editá-los. O loop Ralph (`.claude/scripts/ralph/CLAUDE.md`) **não** lê diagramas durante iteração.

## 9. Success Metrics

| Métrica | Alvo MVP |
|---|---|
| Linhas de código de usuário para CRUD em Lambda + OpenAPI | ≤ 50 |
| Cold start (hello-world, ARM64, 128MB) | < 50 ms |
| Tamanho do binário Lambda (stripped) | < 10 MB |
| Tempo de scaffolding (`serverust new` → `cargo build`) | < 60 s |
| Overhead vs Axum puro (req/s, same workload) | ≤ 5 % |
| Cobertura de testes do framework | ≥ 80 % |
| Endpoints OpenAPI documentados automaticamente | 100 % |

## 10. Open Questions

> As 10 questões originais foram fechadas durante a fase de discovery. Mantidas abaixo como **Decision Log** para rastreabilidade. Novas dúvidas que surgirem durante a implementação devem ser adicionadas ao final.

### Decision Log (fechadas)

| # | Tema | Decisão |
|---|---|---|
| 1 | Macros vs builders | Macros declarativas para rotas e DTOs (`#[get]`, `#[derive(Validate)]`); **DI e App via builders explícitos**. |
| 2 | OpenAPI: utoipa vs aide | **utoipa**. Maturidade, derive macros estáveis, `utoipa-axum`. |
| 3 | DI strategy | **Híbrido**: `Arc<dyn Trait>` default, opt-in para generics via `#[injectable(static)]`. |
| 4 | Idempotência storage | Trait `IdempotencyStore` com **DynamoDB default**; impls custom (Redis/Postgres) pluggable. |
| 5 | CLI lang | **Rust puro com `clap`**. Sem dependência Node. |
| 6 | Naming | **Codename interno "RustAPI"**; nome público definido próximo do release. |
| 7 | MSRV | **Rust 1.85+ (Edition 2024)**. Alinhado a `aws-lambda-rust-runtime` 1.84+ e Lambda Managed Instances. |
| 8 | Hot reload | **`cargo-watch`** no MVP. `bacon` no roadmap pós-MVP. |
| 9 | Powertools scope MVP | Logger + Tracing + Metrics no MVP. **Idempotência completa em fase 2** (trait exposta, macro `#[idempotent]` adiada). |
| 10 | Escape hatch Axum | **`App::axum_router()`** exposto como API pública estável. |

### Novas questões em aberto (durante implementação)

_(adicionar conforme surgirem)_
