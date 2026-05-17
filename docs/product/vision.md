# Visão de Produto — serverust

> Última atualização: 2026-05-16

## O que é

serverust é um framework Rust opinativo para APIs HTTP e AWS Lambda, inspirado em três referências:

- **Axum + Tokio + Tower** como motor — performance nativa, cold start baixo, segurança em compile-time
- **FastAPI** como filosofia de DX — rotas declarativas, validação automática, OpenAPI gerado dos tipos, pouca cerimônia
- **NestJS** como arquitetura — DI, Guards, Pipes, Interceptors, CLI com `new`/`generate`/`dev`/`deploy`

A frase-síntese: **"a experiência do FastAPI + a arquitetura do NestJS, rodando na performance e segurança do Rust."**

## Por que existe

Axum puro é excelente como primitivo, mas exige muito boilerplate para produção: validação, OpenAPI, DI, tratamento de erros e integração Lambda precisam ser montados manualmente a cada projeto. FastAPI e NestJS resolvem isso em suas stacks, mas não existem em Rust com suporte nativo a Lambda.

serverust fecha esse gap: a mesma aplicação roda em Lambda (via `lambda_http`) ou em servidor HTTP local, sem alteração de código de negócio.

## Objetivos mensuráveis

| Objetivo | Métrica |
|---|---|
| API CRUD funcional em Lambda com OpenAPI automático | < 50 linhas de código de usuário |
| Cold start Lambda ARM64 128MB | < 50 ms p95 |
| Binário stripped (`hello-world`) | < 10 MB |
| Onboarding até primeiro endpoint funcionando | < 5 min (getting-started.md) |

## Princípios de design

**Compile-time sobre runtime.** Introspecção de rotas, schema OpenAPI e tabelas de providers são resolvidas em compile-time. Zero overhead runtime, tipagem forte, mensagens de erro claras.

**Macros como açúcar, builders como base.** `#[get]`, `#[post]`, `#[subscriber]` reduzem boilerplate, mas sempre há um builder programático equivalente por baixo. DI e composição de `App` ficam explícitas — evitar "magic" que dificulta debug.

**Estende Axum, não esconde.** O escape hatch `App::axum_router()` retorna o `axum::Router` interno. Extractors do Axum (`Path`, `Query`, `State`) são re-exportados diretamente.

**Opt-in para infraestrutura pesada.** Kafka, DynamoDB, rdkafka: todos atrás de feature flags. O crate `serverust-core` nunca depende de eventos ou brokers — invariante verificada em CI.

**Erros padronizados em compile-time.** `#[derive(ApiError)]` com `#[status(N)]` e `#[message("...")]` converte automaticamente para resposta HTTP tipada. Sem `match` manual nos handlers.

## Escopo intencional (não-objetivos)

- ORM proprietário — o framework integra com `sqlx`, `sea-orm`, `diesel`, não compete
- WebSockets, SSE, gRPC — fase 2
- Reflection runtime para DI — Rust não suporta idiomaticamente; usamos macros + builders
- Suporte a outros providers serverless (GCP, Azure) — fase 2

## Referências

- [CHANGELOG.md](../../CHANGELOG.md) — histórico de versões com o que foi entregue
- [product/roadmap.md](roadmap.md) — o que foi construído e o que vem a seguir
- [architecture/overview.md](../architecture/overview.md) — como o framework é estruturado internamente
- [development/decisions/](../development/decisions/) — ADRs das decisões técnicas
