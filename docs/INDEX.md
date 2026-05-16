# Documentação do serverust Framework

Ponto de entrada da documentação técnica. Comece pelo [README.md](../README.md) na raiz para visão geral e quick-start; este índice mapeia a documentação aprofundada.

## Guias (para quem está usando o framework)

- [**guides/getting-started.md**](guides/getting-started.md) — Em 5 minutos: do zero a uma API HTTP rodando local com Swagger UI.
- [**guides/lambda-tutorial.md**](guides/lambda-tutorial.md) — Tutorial completo: construir uma API de tarefas (CRUD com validação, OpenAPI, DI), rodar local e fazer **deploy em AWS Lambda**. Tempo: 30-45 min.
- [**guides/iac-compatibility.md**](guides/iac-compatibility.md) — Contrato oficial de compatibilidade com Serverless Framework, SST, Terraform, SAM/CDK e como validamos isso em testes/CI.

## Produto

- [**product/prd.md**](product/prd.md) — Product Requirements Document completo. 11 user stories, 11 requisitos funcionais, success metrics, Decision Log.
- [**product/stories.json**](product/stories.json) — User stories estruturadas (JSON usado pelo Ralph loop). Útil para integração com ferramentas.
- [**product/competitors/rocket.md**](product/competitors/rocket.md) — Análise de Rocket v0.5.1: pontos fortes, gaps vs serverust e quando cada um faz sentido.
- [**product/competitors/loco.md**](product/competitors/loco.md) — Análise de Loco.rs v0.16.3: pontos fortes, gaps vs serverust e quando cada um faz sentido.

## Arquitetura

- [**architecture/overview.md**](architecture/overview.md) — Visão geral curta: crates, princípios, stack interna. Aponta para os diagramas.
- [**architecture/diagrams/**](architecture/diagrams/) — 3 diagramas Excalidraw + PNG:
  - `architecture` — componentes do framework + integrações AWS.
  - `sequence` — fluxo de uma requisição em Lambda.
  - `data-flow` — transformação de dados ao longo do pipeline.

## Desenvolvimento

- [**development/decisions.md**](development/decisions.md) — Decision Log das 10 questões fechadas no PRD + decisões adicionais descobertas durante a implementação.
- [**development/decisions/**](development/decisions/) — ADRs no formato MADR 4.0: 0001 (HTTP-first), 0002 (DynamoDB opt-in), 0003 (serverust-events), 0004 (rdkafka opt-in), 0005 (baselines).
- [**development/ralph-progress.md**](development/ralph-progress.md) — Log de execução do Ralph Loop. Codebase Patterns no topo + relato detalhado de cada user story (US-001 a US-011) com aprendizados.

## Guia para Contribuidores e AI Agents

- [**../AGENTS.md**](../AGENTS.md) — Invariantes públicos, processo de release, quality gates, checklist de merge.

## Como navegar

- **Usando o framework pela primeira vez?** [guides/getting-started.md](guides/getting-started.md) → [guides/lambda-tutorial.md](guides/lambda-tutorial.md).
- **Quer entender a arquitetura?** [architecture/overview.md](architecture/overview.md) → diagramas em [architecture/diagrams/](architecture/diagrams/).
- **Contribuindo com código?** [development/decisions.md](development/decisions.md) (padrões fechados) → [development/ralph-progress.md](development/ralph-progress.md) (exemplos do que funcionou).
- **Entendendo o "porquê"?** [product/prd.md](product/prd.md), especialmente seções 6 (Design Considerations) e 7 (Technical Considerations).
- **Referência completa de API?** `cargo doc --workspace --no-deps --open`.

## Histórico de versões

- [**../CHANGELOG.md**](../CHANGELOG.md) — Histórico completo de mudanças por versão (Keep a Changelog 1.1.0).

## Sobre estes artefatos

Os documentos em `docs/` são o **snapshot publicável** do MVP do framework. Foram gerados a partir do PRD original via Ralph Loop e estão consolidados aqui para a comunidade.

Edições humanas vão direto em `docs/`. Diagramas `.excalidraw` podem ser editados em [excalidraw.com](https://excalidraw.com) ou no app desktop — os PNGs ao lado servem só como preview.
