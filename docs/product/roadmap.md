# Roadmap — serverust

> Última atualização: 2026-05-27  
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

## v0.3.0 — SQS maduro (entregue)

`serverust-events 0.3.0` — adapter SQS com paridade de DX ao Kafka: mesma macro `#[subscriber]`, mesmo `EventRouter`, transport abstraction.

**Entregue:**

- `SqsBroker` — Lambda ESM com `ReportBatchItemFailures`; routing por nome de fila no ARN
- `StandaloneSqsBroker` — long-poll ECS/EC2 com graceful shutdown
- `#[subscriber(driver = "sqs", queue = "...")]` + `retry`/`dlq`/`fifo` declarativos
- Extractors `SqsMetadata`, `SqsFifoMetadata`, pipeline Tower (idempotency, heartbeat, EMF/X-Ray)
- `SqsProducer` / FIFO type-state; `serverust queue inspect|tail`
- Guia: [event-driven.md](../guides/event-driven.md) (secção SQS); pesquisa: [sqs-inspiration-tier-list.md](../research/sqs-inspiration-tier-list.md)

**Patches pós-0.3.0 (entregues em `main`):**

- Ack silencioso corrigido: fila sem handler ou ARN inválido → `batch_item_failures` quando há `message_id`
- Backoff exponencial no `EventRouter`: expoente limitado + `Duration::saturating_mul` (sem panic em overflow)

---

## Próximas versões (planejamento)

### v0.3.x — Confiabilidade event-driven (restante)
- **Retry topics físicos** (Kafka): tópicos de retry com backoff configurável
- **Outbox pattern**: gravar evento na mesma transação do banco; worker dispara depois
- **Correlation IDs automáticos**: propagados em headers de cada mensagem

### v0.3.1 — Hardening do SqsBroker (follow-up)

Itens ainda abertos da review inicial (patches de ack/backoff no router já entregues — ver **Patches pós-0.3.0** acima):

- **Structured logging**: `tracing::error!` para falhas de invariante com campos `queue`, `message_id`, `attempt`, `error`
- **Schema validation pós-deserialização** em `Json<T>` e `SqsMetadata::from_message`
- **Graceful degradation**: contador `idempotency_bypass_total` quando `message_id` vazio; validação básica de formato em `receipt_handle` antes do heartbeat
- **Overflow protection no backoff (SQS layers)**: saturação + `max_backoff: Duration` configurável (default 30s) em `SqsProducer` / `DeleteManager` / `RetryLayer`
- **Métricas EMF**: `idempotency_bypass_total`, `metadata_serialize_failures_total`, `heartbeat_invalid_receipt_total`
- **Circuit breaker** no `StandaloneSqsBroker` após falhas consecutivas de `ReceiveMessage`
- **`max_backoff` configurável** no retry da macro (hoje: cap de expoente em 31 no router programático)

### v0.4 — Multi-Lambda scaffolding + transport abstraction completa

Inspiração: SST, AWS SAM, Encore.ts, Cargo Lambda. Pesquisa em `docs/research/multi-lambda-tier-list.md` (a criar).

- **`serverust new project <name> --multi-lambda`**: scaffold canônico `functions/<name>/` + `crates/shared/` + `crates/events/` + `infra/` + `serverust.toml` (manifesto) + Dockerfile único.
- **Manifesto `serverust.toml`**: source-of-truth para queues, topics, buckets, schedules (Encore-style); macros `#[subscriber]/#[publisher]/#[get]` declaram handlers que consomem (híbrido manifest + decorator).
- **Geradores IaC plugáveis** via trait `IaCGenerator`: Terraform + SAM + SST como built-in v1; CDK, Pulumi, Serverless Framework como extension points entregues em v0.4.x.
- **CLI commands**: `serverust new lambda <name> --trigger sqs|http|sns|s3|schedule`, `serverust list`, `serverust diagram` (gera excalidraw da topologia), `serverust dev` (watch + invoke local), `serverust deploy --stage <env>`.
- **LMI (Lambda Managed Instances)**: flag `lmi = true` no manifesto — Rust é particularmente bom para LMI (persistent warm instances).
- **AsyncAPI como contract registry inter-lambda**: schemas em `crates/events/` viram source-of-truth tipado.

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
- [ADRs](../development/decisions/) — decisões arquiteturais registradas
- [Análises competitivas](competitors/) — Axum, actix-web, Rocket, Loco
