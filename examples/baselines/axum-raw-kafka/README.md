# axum-raw-kafka — Baseline competitivo vanilla

Este diretório contém a implementação **vanilla** equivalente ao `examples/kafka-wallet` do serverust, mas **sem usar nenhuma abstração do framework** — apenas `axum`, `rdkafka`, `aws-sdk-dynamodb` e `lambda_runtime` crus.

Seu único propósito é servir de base de comparação para o benchmark competitivo do serverust.

## Por que existe

O script `scripts/benchmark_competitive.sh` mede LOC do handler e tamanho de binário lado a lado:

| Métrica | serverust (kafka-wallet) | axum-raw-kafka (baseline) | Ratio |
|---|---|---|---|
| LOC handler | 16 | 64 | 4,0× menos |

Esses números são citados no `docs/product/competitors/release-competitive-log.md` e no `README.md` principal do serverust.

## O que o handler faz

Mesmo fluxo que o `kafka-wallet` do serverust:
1. Recebe evento Kafka via Lambda trigger.
2. Lê/atualiza saldo da carteira no DynamoDB.
3. Publica resultado no tópico `wallet.results`.

## Como buildar (para benchmark)

Este projeto não faz parte do workspace Cargo principal (`publish = false`, `workspace` próprio). Para buildá-lo:

```bash
cd examples/baselines/axum-raw-kafka
cargo build --release
```

Requer `librdkafka` (e `libcurl`) instalado no sistema. Em ambientes de CI sem essas libs, o build falha — isso é esperado e documentado no `release-competitive-log.md`.

## Não use este código em produção

É propositalmente sem abstrações, sem retry, sem tratamento de erro robusto. O objetivo é medir o custo de boilerplate — não demonstrar boas práticas.
