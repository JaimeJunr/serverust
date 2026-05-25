# Roadmap — serverust

> Última atualização: 2026-05-25  
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

### Docs e decisões
- ADRs: [0006 — rust-rdkafka vs RSKafka](../development/decisions/0006-rdkafka-vs-rskafka.md), [0007 — design da API event-driven](../development/decisions/0007-event-api-design-macro-builder.md)
- Guias: [event-driven.md](../guides/event-driven.md), [dynamodb.md](../guides/dynamodb.md)
- Análises competitivas: [axum.md](competitors/axum.md), [actix.md](competitors/actix.md) (atualizado v4.13.0)

---

## v0.3.0 — SqsBroker maduro (entregue)

Confiabilidade event-driven com SQS sem quebrar o pitch HTTP-first: tudo atrás de feature `sqs` em `serverust-events`.

**Entregue:**

- `SqsBroker` para Lambda Event Source Mapping com `SqsBatchResponse.batchItemFailures`
- `StandaloneSqsBroker` para workers ECS/EC2/bare-metal com long-poll, delete batch e graceful shutdown
- Extractors SQS: `SqsMetadata` e `SqsFifoMetadata`
- `SqsProducer` com batching, retry em falhas parciais e shutdown gracioso
- `SqsFifoProducer` com builder type-state exigindo `message_group_id` antes de `send()`
- Macro `#[subscriber(driver = "sqs", queue = "...")]`, incluindo `fifo`, `retry`, `dlq` e `asyncapi`
- Tower pipeline SQS: tracing, idempotência, DLQ e retry
- AsyncAPI 3.0 via `AsyncApiBuilder`, `#[subscriber(..., asyncapi)]` e `serverust info --asyncapi`
- CLI `serverust queue inspect/tail` para diagnóstico básico de filas SQS

**Preservado:**

- `serverust-core` continua sem deps de SQS/Kafka/eventos
- `examples/hello-world` continua sem deps transitivas de SQS
- SQS e AsyncAPI continuam opt-in por feature

---

## Próximas versões (planejamento)

### v0.3.1 — Hardening do SqsBroker (follow-up review PR #5)

Sugestões da review automática (Claude Code Action) agendadas para release de patch após v0.3.0:

- **Structured logging completo**: padronizar `tracing::warn!` para `tracing::error!` quando o evento é falha de invariante. Campos consistentes: `queue`, `message_id`, `attempt`, `error`.
- **Schema validation pós-deserialização**: validar campos obrigatórios em `Json<T>` extractor e em `SqsMetadata::from_message`.
- **Graceful degradation**: contador `idempotency_bypass_total` quando `message_id` vazio; validação básica de formato em `receipt_handle` antes do heartbeat.
- **Overflow protection no backoff**: trocar `config.base_backoff * 2u32.pow(attempt - 1)` por `saturating_pow` + `max_backoff: Duration` configurável (default 30s).
- **Métricas EMF operacionais**: `idempotency_bypass_total`, `metadata_serialize_failures_total`, `heartbeat_invalid_receipt_total`.
- **Circuit breaker no `StandaloneSqsBroker`**: trip após N falhas consecutivas de `ReceiveMessage` para evitar storm em incidente AWS.

### v0.4 — Multi-Lambda scaffolding + transport abstraction completa

Inspiração: SST, AWS SAM, Encore.ts, Cargo Lambda. Pesquisa em `docs/research/multi-lambda-tier-list.md` (a criar).

- **`serverust new project <name> --multi-lambda`**: scaffold canônico `functions/<name>/` + `crates/shared/` + `crates/events/` + `infra/` + `serverust.toml` (manifesto) + Dockerfile único.
- **Manifesto `serverust.toml`**: source-of-truth para queues, topics, buckets, schedules (Encore-style); macros `#[subscriber]/#[publisher]/#[get]` declaram handlers que consomem (híbrido manifest + decorator).
- **Geradores IaC plugáveis** via trait `IaCGenerator`: Terraform + SAM + SST como built-in v1; CDK, Pulumi, Serverless Framework como extension points entregues em v0.4.x.
- **CLI commands**: `serverust new lambda <name> --trigger sqs|http|sns|s3|schedule`, `serverust list`, `serverust diagram` (gera excalidraw da topologia), `serverust dev` (watch + invoke local), `serverust deploy --stage <env>`.
- **LMI (Lambda Managed Instances)**: flag `lmi = true` no manifesto — Rust é particularmente bom para LMI (persistent warm instances).
- **AsyncAPI como contract registry inter-lambda**: schemas em `crates/events/` viram source-of-truth tipado.
- **Retry topics físicos / outbox pattern**: confiabilidade para fluxos Kafka e workflows cross-service.
- **Correlation IDs automáticos**: propagados em headers de cada mensagem, base para tracing distribuído.

### v0.5 — Observabilidade e contratos
- **Sagas como crate separado** (`serverust-sagas`): state machines para workflows de longa duração
- **Topology declarativa** (inspiração Kafka Streams): `source → filter → map → sink` descritivo

### Futuro / backlog
- WebSockets e Server-Sent Events
- gRPC via tonic adapter
- Suporte a outros brokers: RabbitMQ (`serverust-rabbitmq`), NATS (`serverust-nats`) — `Broker` trait já está pronta
- Suporte a outros providers serverless (GCP Cloud Run, Azure Functions)
- **Remover ignores de RUSTSEC em `deny.toml`** quando `aws-sdk-rust` subir para `rustls 0.23+`: RUSTSEC-2026-0098, 0099, 0104 (rustls-webpki 0.101.7 transitive via aws-smithy-http-client). Acompanhar https://github.com/awslabs/aws-sdk-rust/issues.

---

## Referências de design

- [Tier list de inspirações Kafka](../research/kafka-inspiration-tier-list.md) — FastStream, MassTransit, NestJS Microservices, Spring Kafka
- [Tier list de inspirações SQS](../research/sqs-inspiration-tier-list.md) — Lambda ESM, workers standalone, DLQ, idempotência e FIFO
- [ADRs](../development/decisions/) — decisões arquiteturais registradas
- [Análises competitivas](competitors/) — Axum, actix-web, Rocket, Loco
