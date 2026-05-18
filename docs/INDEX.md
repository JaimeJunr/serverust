# Documentação do serverust Framework

Ponto de entrada da documentação técnica. Comece pelo [README.md](../README.md) na raiz para visão geral e quick-start; este índice mapeia a documentação aprofundada.

## Guias (para quem está usando o framework)

- [**guides/getting-started.md**](guides/getting-started.md) — Em 5 minutos: do zero a uma API HTTP rodando local com Swagger UI.
- [**guides/lambda-tutorial.md**](guides/lambda-tutorial.md) — Tutorial completo: construir uma API de tarefas (CRUD com validação, OpenAPI, DI), rodar local e fazer **deploy em AWS Lambda**. Tempo: 30-45 min.
- [**guides/iac-compatibility.md**](guides/iac-compatibility.md) — Contrato oficial de compatibilidade com Serverless Framework, SST, Terraform, SAM/CDK e como validamos isso em testes/CI.
- [**guides/dynamodb.md**](guides/dynamodb.md) — Guia prático de DynamoDB: setup de deps, CRUD com `DynamoRepo<T>` e `#[dynamo_table]`, credenciais (env vars, IAM role em Lambda), testes locais com DynamoDB Local e troubleshooting.
- [**guides/event-driven.md**](guides/event-driven.md) — Guia de event-driven com Kafka: `Broker` trait, `EventRouter`, macros `#[subscriber]`/`#[publisher]`, retry policies e detecção de runtime Lambda vs long-running.

## Produto

- [**product/vision.md**](product/vision.md) — O que é, por que existe, objetivos mensuráveis, princípios de design e não-objetivos.
- [**product/roadmap.md**](product/roadmap.md) — O que foi entregue (v0.1.x, v0.2.0) e o que está planejado (v0.3, v0.4, backlog).
- [**product/competitors/axum.md**](product/competitors/axum.md) — Análise de Axum: mesma stack Tower/Tokio, gaps em DI nativa, OpenAPI e Lambda.
- [**product/competitors/actix.md**](product/competitors/actix.md) — Análise de actix-web v4.13.0: pontos fortes, gaps vs serverust e quando cada um faz sentido.
- [**product/competitors/rocket.md**](product/competitors/rocket.md) — Análise de Rocket v0.5.1: pontos fortes, gaps vs serverust e quando cada um faz sentido.
- [**product/competitors/loco.md**](product/competitors/loco.md) — Análise de Loco.rs v0.16.3: pontos fortes, gaps vs serverust e quando cada um faz sentido.
- [**product/metrics/**](product/metrics/) — SLOs publicados e histórico de benchmarks por versão.

## Arquitetura

- [**architecture/overview.md**](architecture/overview.md) — Visão geral curta: crates, princípios, stack interna. Aponta para os diagramas.
- [**architecture/diagrams/**](architecture/diagrams/) — 3 diagramas Excalidraw + PNG:
  - `architecture` — componentes do framework + integrações AWS.
  - `sequence` — fluxo de uma requisição em Lambda.
  - `data-flow` — transformação de dados ao longo do pipeline.

## Desenvolvimento

- [**development/decisions.md**](development/decisions.md) — Decision Log das 10 questões fechadas no PRD + decisões adicionais descobertas durante a implementação.
- [**development/decisions/**](development/decisions/) — ADRs no formato MADR 4.0: 0001 (HTTP-first), 0002 (DynamoDB opt-in), 0003 (serverust-events), 0004 (rdkafka opt-in), 0005 (baselines), 0006 (rdkafka vs RSKafka), 0007 (event API design).
- [**development/release-checklist.md**](development/release-checklist.md) — Checklist de release e runbook dos workflows de CI/release (`release-plz`, cocogitto, nextest, cargo-deny, cargo-machete).
- [**development/ralph-progress.md**](development/ralph-progress.md) — Learnings de implementação por versão. Codebase Patterns + relato detalhado por user story com o que funcionou e o que não funcionou.

## Guia para Contribuidores e AI Agents

- [**../CLAUDE.md**](../CLAUDE.md) — Invariantes públicos, processo de release, quality gates, checklist de merge.

## Como navegar

- **Usando o framework pela primeira vez?** [guides/getting-started.md](guides/getting-started.md) → [guides/lambda-tutorial.md](guides/lambda-tutorial.md).
- **Quer entender a arquitetura?** [architecture/overview.md](architecture/overview.md) → diagramas em [architecture/diagrams/](architecture/diagrams/).
- **Contribuindo com código?** [development/decisions/](development/decisions/) (ADRs) → [development/ralph-progress.md](development/ralph-progress.md) (learnings de implementação).
- **Entendendo o "porquê"?** [product/vision.md](product/vision.md) (filosofia e objetivos) → [product/roadmap.md](product/roadmap.md) (o que foi construído e por quê).
- **Referência completa de API?** `cargo doc --workspace --no-deps --open`.

## Histórico de versões

- [**../CHANGELOG.md**](../CHANGELOG.md) — Histórico completo de mudanças por versão (Keep a Changelog 1.1.0).

## Sobre esta documentação

Os documentos em `docs/` são a documentação pública do framework — visão de produto, guias de uso, arquitetura e learnings de implementação.

Artefatos de planejamento (PRDs, stories JSON, progress logs brutos) ficam em `.ralph/` e não pertencem aqui. Edições humanas vão direto em `docs/`. Diagramas `.excalidraw` podem ser editados em [excalidraw.com](https://excalidraw.com) ou no app desktop — os PNGs ao lado servem só como preview.
