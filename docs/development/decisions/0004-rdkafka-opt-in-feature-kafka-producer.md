# ADR 0004 — rdkafka opt-in atrás de feature kafka-producer

- **Status:** Accepted
- **Date:** 2025-01-01 (prospectiva — decisão do v0.2.0)
- **Deciders:** maintainers serverust

---

## Contexto e Problema

`rdkafka` é um binding C/C++ (librdkafka) que aumenta o binário em vários MB e adiciona dependência de compilação nativa. Nem todos os consumers Kafka precisam produzir mensagens — o Lambda pode só consumir e fazer PUT no DynamoDB.

## Drivers de Decisão

- Invariante pública: binário `hello-world` < 10 MB (feature off por default garante isso).
- Binário `kafka-wallet` (com producer) ≤ 12 MB — limite aceito para o caso de uso completo.
- `rdkafka` exige `cmake` e libs C na build — não deve ser transitivo para quem só consome.
- Consumer Kafka em Lambda usa `aws_lambda_events::kafka::KafkaEvent` — sem rdkafka, sem dep nativa.

## Opções Consideradas

1. **`rdkafka` atrás de `feature = "kafka-producer"` em `serverust-events`** (escolhida)
2. `rdkafka` como dep obrigatória de `serverust-events`
3. Crate separada `serverust-kafka-producer`

## Decisão

Em `serverust-events/Cargo.toml`:

```toml
[features]
default = []
kafka = []          # KafkaRecord<T> decoder — sem rdkafka, zero dep nativa
kafka-producer = ["kafka", "dep:rdkafka", "dep:aws-msk-iam-sasl-signer"]
```

- **`feature = "kafka"`**: só `aws_lambda_events` — consumer Kafka puro, sem deps nativas.
- **`feature = "kafka-producer"`**: ativa `rdkafka` + IAM SASL signer — para quem precisa publicar.

Autenticação: IAM SASL via `aws-msk-iam-sasl-signer` como padrão para MSK; fallback SASL_SSL plain quando `MSK_IAM_ROLE` ausente.

## Consequências

### Positivas
- Consumer-only Lambda fica enxuto (sem rdkafka, sem cmake).
- `kafka-wallet` aceita 12 MB por ser o exemplo completo com producer.
- `hello-world` e apps HTTP-only ficam completamente desacoplados de Kafka.

### Negativas / Trade-offs
- Dois feature flags para Kafka — usuário precisa saber a diferença.
- `rdkafka` requer libssl e cmake no ambiente de build para CI.

## Verificação

```bash
# consumer-only: sem rdkafka
cargo tree -p serverust-events | grep rdkafka  # deve ser vazio sem --features kafka-producer

# com producer
cargo tree -p serverust-events --features kafka-producer | grep rdkafka  # deve listar
```

## Links e Referências

- ADR 0003 — `serverust-events` como crate separada
- [`../../CLAUDE.md`](../../CLAUDE.md) — invariante de tamanho de binário
- [rdkafka crate](https://crates.io/crates/rdkafka)
- [aws-msk-iam-sasl-signer](https://crates.io/crates/aws-msk-iam-sasl-signer)
