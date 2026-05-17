# Roadmap — serverust

> Última atualização: 2026-05-16  
> Para o histórico detalhado de mudanças por versão, veja [CHANGELOG.md](../../CHANGELOG.md).

---

## v0.1.x — HTTP-first + Lambda (entregue)

Fundação do framework: roteamento declarativo, DI, OpenAPI automático, runtime dual Lambda/HTTP.

**Entregue:**
- Macros `#[get]`, `#[post]`, `#[put]`, `#[delete]`, `#[patch]` com extractors tipados
- `App` builder + DI via `App::provide::<T>(Arc<T>)` + `State<Arc<T>>` nos handlers
- OpenAPI 3.1 automático via utoipa + Swagger UI em `/docs` + ReDoc em `/redoc`
- `#[derive(ApiError)]` com `#[status(N)]` e `#[message("...")]` → resposta HTTP tipada
- Validação automática com `#[derive(Validate)]` → 422 padronizado
- Guards, Pipes e Interceptors
- Runtime dual: `App::run()` detecta Lambda (`AWS_LAMBDA_RUNTIME_API`) vs HTTP local
- Telemetria: logger JSON estruturado, tracing X-Ray, métricas EMF, `IdempotencyStore`
- CLI `serverust-cli`: `new`, `generate`, `dev`, `build`, `deploy lambda`, `info`, `openapi export`
- Exemplos: `hello-world`, `funds-api`, `kafka-wallet` (básico), `todo-api`

**SLOs publicados (invariantes de CI):**
- Cold start ARM64 128MB (`hello-world`) < 50 ms p95
- Binário stripped < 10 MB
- `serverust-core` sem deps de Kafka/event

---

## v0.2.0 — Event-driven com Kafka (entregue)

Evolução do `serverust-events` de extractor simples para framework event-driven completo, com abstração de broker e DX inspirada em FastStream + MassTransit + NestJS Microservices.

**Entregue:**

### Broker abstraction
- Trait `Broker` (`subscribe`, `publish`) — desacopla a API pública do transport concreto
- `KafkaBroker` (rust-rdkafka) atrás de feature `kafka` — ~1M msgs/s, exactly-once semantics
- `LambdaBroker` para trigger MSK em Lambda — despacha `aws_lambda_events::KafkaEvent` sem rdkafka
- `InMemoryBroker` (feature `in-memory`) — testes sem infra, sem deps extras

### EventRouter e builder
- `EventRouter::new().subscribe::<T, _>(topic, handler)` — builder fluente, Axum-like
- `.with_retry(RetryPolicy::exponential(3, Duration::from_secs(1)))` — retry composável
- `.with_dlq("topic.dlq")` — dead letter queue
- `RetryPolicy`: `immediate(n)`, `exponential(n, delay)`, `.dead_letter(topic)`

### Extractors e macros
- Extractors tipados: `KafkaHeaders`, `EventCtx` (topic, partition, offset, timestamp), `State<S>`
- `#[subscriber(topic = "...")]` — macro de atributo, gera código do builder
- `#[publisher(topic = "...")]` — empilhável sobre subscriber, publica o valor de retorno
- Detecção automática Lambda vs long-running via `AWS_LAMBDA_FUNCTION_NAME`

### AsyncAPI e docs
- Feature `asyncapi`: schema AsyncAPI 3.0 gerado dos tipos Rust
- `serverust info --asyncapi` emite YAML AsyncAPI sem subir o consumer
- `serverust-cli`: novo subcomando `asyncapi export --out asyncapi.yaml`
- ADRs: [0006 — rust-rdkafka vs RSKafka](../development/decisions/0006-rdkafka-vs-rskafka.md), [0007 — design da API event-driven](../development/decisions/0007-event-api-design-macro-builder.md)
- Guias: [event-driven.md](../guides/event-driven.md), [dynamodb.md](../guides/dynamodb.md)
- Análises competitivas: [axum.md](competitors/axum.md), [actix.md](competitors/actix.md) (atualizado v4.13.0)

---

## Próximas versões (planejamento)

### v0.3 — Confiabilidade event-driven
- **Retry topics físicos**: tópicos Kafka de retry com backoff configurável (padrão Spring Kafka enterprise)
- **Outbox pattern**: gravar evento na mesma transação do banco; worker dispara depois — sem perda mesmo com rollback
- **Correlation IDs automáticos**: propagados em headers de cada mensagem, base para tracing distribuído

### v0.4 — Observabilidade e contratos
- **Sagas como crate separado** (`serverust-sagas`): state machines para workflows de longa duração
- **Topology declarativa** (inspiração Kafka Streams): `source → filter → map → sink` descritivo

### v0.3.1 — Hardening do SqsBroker (follow-up review PR #5)

Sugestões da review automática (Claude Code Action) agendadas para release de patch após v0.3.0:

- **Structured logging completo**: padronizar todos os `tracing::warn!` do módulo `sqs` para `tracing::error!` quando o evento é uma falha de invariante (vs. degradação esperada). Campos consistentes: `queue`, `message_id`, `attempt`, `error`.
- **Schema validation pós-deserialização**: validar campos obrigatórios em `Json<T>` extractor e em `SqsMetadata::from_message` — hoje só verificamos parse, não shape mínimo.
- **Graceful degradation em edge cases**:
  - `message_id` vazio: continuar gerando métrica `idempotency_bypass_total` + ainda emite warning (atual).
  - `receipt_handle` inválido: validar formato básico antes de spawn do heartbeat (evita chamada AWS desnecessária).
- **Overflow protection no backoff exponencial**: trocar `config.base_backoff * 2u32.pow(attempt - 1)` por `saturating_pow` + cap máximo configurável (`max_backoff: Duration`, default 30s).
- **Métricas operacionais**: contadores EMF para `idempotency_bypass_total`, `metadata_serialize_failures_total`, `heartbeat_invalid_receipt_total` — alimentam dashboard CloudWatch e alertam config errada.
- **Circuit breaker no `StandaloneSqsBroker`**: tripping após N falhas consecutivas de `ReceiveMessage` (ex: 5 erros em 1 min ⇒ pausa de 30s) para evitar storm em incidente AWS.

### Futuro / backlog
- WebSockets e Server-Sent Events
- gRPC via tonic adapter
- Suporte a outros brokers: RabbitMQ (`serverust-rabbitmq`), NATS (`serverust-nats`) — `Broker` trait já está pronta
- Suporte a outros providers serverless (GCP Cloud Run, Azure Functions)

---

## Referências de design

- [Tier list de inspirações Kafka](../research/kafka-inspiration-tier-list.md) — FastStream, MassTransit, NestJS Microservices, Spring Kafka
- [ADRs](../development/decisions/) — decisões arquiteturais registradas
- [Análises competitivas](competitors/) — Axum, actix-web, Rocket, Loco
