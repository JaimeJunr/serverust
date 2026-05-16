# ADR 0003 — Event-driven em crate separada serverust-events, NUNCA em serverust-core

- **Status:** Accepted
- **Date:** 2025-01-01 (prospectiva — decisão do v0.2.0)
- **Deciders:** maintainers serverust

---

## Contexto e Problema

v0.2.0 adiciona suporte a event sources não-HTTP (Kafka, SQS, EventBridge, S3). Era necessário decidir onde colocar esse código sem violar o princípio HTTP-first e sem poluir `serverust-core`.

## Drivers de Decisão

- Invariante pública: `serverust-core` não pode ter deps de Kafka, rdkafka ou qualquer event broker.
- `serverust-core` é a dep base de todos os projetos serverust — qualquer adição tem custo universal.
- Usuários HTTP-only não devem pagar pelo event system em tamanho de binário, tempo de compilação ou complexidade.
- A abstração `EventHandler<E>` deve ser reutilizável para múltiplos event sources (Kafka, SQS, etc.).

## Opções Consideradas

1. **Nova crate `serverust-events` com feature flags por source** (escolhida)
2. Adicionar event handling em `serverust-core` atrás de features
3. Adicionar event handling em `serverust-lambda` (já tinha o adapter Lambda)

## Decisão

Criar `serverust-events` como crate independente no workspace com:

- `EventHandler<E>` trait (import de `serverust-core` para reusar `Container`/`State`)
- `KafkaRecord<T>` extractor atrás de feature `kafka` (default = off)
- `KafkaProducer` atrás de feature `kafka-producer` (default = off)
- **`serverust-core` NUNCA importa `serverust-events`** — dependência é unidirecional

Esta regra é invariante e requer nova ADR para ser relaxada.

## Consequências

### Positivas
- `serverust-core` permanece limpo e leve para todos os usuários HTTP-only.
- `serverust-events` pode evoluir com novos sources (SQS, EventBridge) sem risco de regressão HTTP.
- Usuários Kafka adicionam só `serverust-events = { features = ["kafka"] }`.

### Negativas / Trade-offs
- Mais um crate para manter e publicar no crates.io.
- Integração DI entre `serverust-core` e `serverust-events` requer cuidado (import de `Container` sem criar dependência circular).

## Verificação

```bash
# deve retornar vazio
cargo tree -p serverust-core | grep -E "kafka|rdkafka|event"
```

## Links e Referências

- ADR 0001 — HTTP-first (princípio preservado por esta decisão)
- ADR 0004 — rdkafka opt-in
- [`../../CLAUDE.md`](../../CLAUDE.md) — invariante `serverust-core sem deps de eventos/Kafka`
