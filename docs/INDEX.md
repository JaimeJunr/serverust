# Documentação do RustAPI Framework

Ponto de entrada da documentação técnica. Comece pelo [README.md](../README.md) na raiz para visão geral e quick-start; este índice mapeia a documentação aprofundada.

## Produto

- [**produt/prd.md**](product/prd.md) — Product Requirements Document completo. 11 user stories, 11 requisitos funcionais, success metrics, Decision Log.
- [**product/stories.json**](product/stories.json) — User stories estruturadas (JSON usado pelo Ralph loop). Útil para integração com ferramentas.

## Arquitetura

- [**architecture/overview.md**](architecture/overview.md) — Visão geral curta: crates, princípios, stack interna. Aponta para os diagramas.
- [**architecture/diagrams/**](architecture/diagrams/) — 3 diagramas Excalidraw + PNG:
  - `architecture` — componentes do framework + integrações AWS.
  - `sequence` — fluxo de uma requisição em Lambda.
  - `data-flow` — transformação de dados ao longo do pipeline.

## Desenvolvimento

- [**development/decisions.md**](development/decisions.md) — Decision Log das 10 questões fechadas no PRD + decisões adicionais descobertas durante a implementação.
- [**development/ralph-progress.md**](development/ralph-progress.md) — Log de execução do Ralph Loop. Codebase Patterns no topo + relato detalhado de cada user story (US-001 a US-011) com aprendizados.

## Como navegar

- **Começando do zero?** Leia [README.md](../README.md) → [architecture/overview.md](architecture/overview.md).
- **Implementando uma feature nova?** Consulte [development/decisions.md](development/decisions.md) (padrões a seguir) → seções específicas de [development/ralph-progress.md](development/ralph-progress.md) (exemplos do que funcionou).
- **Entendendo o "porquê"?** [product/prd.md](product/prd.md), especialmente seções 6 (Design Considerations) e 7 (Technical Considerations).
- **Editando diagramas?** Abra os arquivos `.excalidraw` em [excalidraw.com](https://excalidraw.com) ou no app desktop. Os PNGs são gerados pela skill `excalidraw-diagram` (re-renderizar após edição).

## Sobre estes artefatos

Os documentos em `docs/` são o **snapshot publicável** do MVP do framework. Foram gerados a partir do PRD original via Ralph Loop e estão consolidados aqui para a comunidade.

Edições humanas vão direto em `docs/`. Diagramas `.excalidraw` podem ser editados em [excalidraw.com](https://excalidraw.com) ou no app desktop — os PNGs ao lado servem só como preview.
