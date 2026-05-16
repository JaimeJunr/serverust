# ADR 0005 — Baseline competitivo no próprio repo em examples/baselines/

- **Status:** Accepted
- **Date:** 2025-01-01 (prospectiva — decisão do v0.2.0)
- **Deciders:** maintainers serverust

---

## Contexto e Problema

O pitch competitivo do serverust depende de números reais e auditáveis (LOC, cold start, tamanho de binário). Comparações "a gente mediu internamente" são difíceis de verificar pela comunidade. Era necessário uma estratégia para tornar os benchmarks reproduzíveis.

## Drivers de Decisão

- Números competitivos devem ser auditáveis por qualquer contribuidor ou usuário cético.
- O baseline deve ser equivalente funcional ao exemplo serverust (mesmo fluxo de negócio).
- Manter o baseline fora do repo principal aumenta o risco de desatualização.
- `publish = false` garante que baselines não vão para crates.io como "recomendações".

## Opções Consideradas

1. **`examples/baselines/<nome>/` no workspace com `publish = false`** (escolhida)
2. Repositório externo `serverust-benchmarks`
3. Gist ou documento sem código executável

## Decisão

Baselines ficam em `examples/baselines/` com:

- `Cargo.toml` com `publish = false` — nunca vão para crates.io.
- **Zero deps de `serverust-*`** — implementação vanilla com as mesmas libs que qualquer dev usaria.
- `README.md` explicitando: "Este não é exemplo de uso recomendado. É referência de benchmark."
- `scripts/benchmark_competitive.sh` mede ambos (serverust example + baseline) e gera JSON validado contra `docs/product/metrics/schema.json`.

Scripts de qualidade (`scripts/quality_coverage.sh`, `scripts/quality_mutation.sh`) **não rodam** nos baselines — são código de benchmark, não produção.

## Consequências

### Positivas
- Qualquer pessoa pode clonar o repo e reproduzir os números com `scripts/benchmark_competitive.sh`.
- CI pode validar que o baseline compila (`cargo build -p axum-raw-kafka`).
- Baseline evolui junto com o exemplo serverust — difícil de desatualizar acidentalmente.

### Negativas / Trade-offs
- Workspace cresce com código que não é framework nem exemplo de uso.
- Baselines precisam ser mantidos quando as libs externas mudam (rdkafka, aws-sdk, etc.).

## Estrutura Esperada

```
examples/baselines/
  axum-raw-kafka/
    Cargo.toml   (publish = false, zero deps serverust-*)
    src/
      main.rs    (decode Base64 manual, tipos próprios, FutureProducer cru)
    README.md    (explica propósito de benchmark)
```

## Links e Referências

- [`docs/product/metrics/history.json`](../../product/metrics/history.json) — onde os números são registrados
- [`docs/product/competitors/release-competitive-log.md`](../../product/competitors/release-competitive-log.md)
- ADR 0003 — separação `serverust-events` (motiva o baseline Kafka)
- [`scripts/benchmark_competitive.sh`](../../../scripts/benchmark_competitive.sh)
