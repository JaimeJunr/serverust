# ADR 0002 — DynamoDB feature opt-in em serverust-telemetry

- **Status:** Accepted
- **Date:** 2024-06-01 (retroativa — decisão do v0.1.x)
- **Deciders:** maintainers serverust

---

## Contexto e Problema

`IdempotencyStore` e `DynamoDbIdempotencyStore` foram implementados durante v0.1.x. A questão era: em qual crate colocar e como isolar a dependência pesada do AWS SDK?

## Drivers de Decisão

- `aws-sdk-dynamodb` adiciona ~ 5–15 MB ao binário stripped — inaceitável como dep obrigatória.
- Usuários que não usam idempotência não devem pagar o custo em tamanho e cold start.
- `serverust-telemetry` já era o crate de "infraestrutura AWS" (logs, tracing, métricas EMF).
- Invariante pública: binário `hello-world` < 10 MB sem nenhuma dep de DynamoDB.

## Opções Consideradas

1. **Feature `dynamodb` opt-in em `serverust-telemetry`** (escolhida)
2. Crate separada `serverust-dynamodb`
3. Dep obrigatória em `serverust-core`

## Decisão

`IdempotencyStore` (trait) fica em `serverust-core`. A implementação concreta `DynamoDbIdempotencyStore` fica em `serverust-telemetry` atrás de `feature = "dynamodb"` (default = off).

Usuários que precisam de idempotência adicionam:
```toml
serverust-telemetry = { version = "...", features = ["dynamodb"] }
```

## Consequências

### Positivas
- `hello-world` e qualquer app sem idempotência DynamoDB mantém binário enxuto.
- Padrão pode ser replicado para outras integrações AWS pesadas.
- Trait pública permite implementações custom (Redis, Postgres) sem mudar `serverust-core`.

### Negativas / Trade-offs
- Feature flags aumentam superfície de teste (combinatória).
- Usuário precisa conhecer o feature name — documentação obrigatória.

## Links e Referências

- Decisão #4 (Idempotência) e #9 (Powertools scope MVP) em [`../decisions.md`](../decisions.md)
- `serverust-telemetry/Cargo.toml` — feature `dynamodb`
- Invariante de binário em [`../../AGENTS.md`](../../AGENTS.md)
