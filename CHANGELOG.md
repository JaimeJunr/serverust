# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- MAINTENANCE: When bumping workspace.version in Cargo.toml, add a new ## [x.y.z] section
     above [Unreleased] with date YYYY-MM-DD and move relevant [Unreleased] entries there. -->

## [Unreleased]

## [0.3.0] - 2026-05-17

`serverust-events 0.3.0` — SqsBroker maduro: Lambda ESM + Standalone worker, FIFO type-safe, Tower pipeline, idempotency, DLQ declarativo, transport abstraction SQS↔Kafka, AsyncAPI, EMF, X-Ray e CLI inspector. 14 user stories (US-001..US-014) entregues.

### Added
- `SqsBroker` em `serverust-events/src/sqs/consumer.rs` (feature `sqs`) — consumer Lambda ESM com partial batch failure automático via `SqsBatchResponse.batchItemFailures` (US-001)
- Macro `#[subscriber(driver = "sqs", queue = "...")]` em `serverust-macros` — mesma macro suporta `driver = "kafka"` e `driver = "sqs"` sem alterar a lógica do handler (US-001, US-011)
- Extractors estilo Axum para SQS em `serverust-events/src/sqs/extract.rs` — `Json<T>`, `State<S>`, `SqsMetadata` (`message_id`, `receipt_handle`, `attributes`, `system_attributes`) (US-002)
- `DeleteManager` em `serverust-events/src/sqs/delete.rs` — agrupa `DeleteMessageBatch` no standalone worker; em Lambda ESM o ack/nack é controlado por `batchItemFailures` (US-003)
- `SqsProducer` em `serverust-events/src/sqs/producer.rs` — batching transparente (até 10 msgs / 200ms linger, configurável), retry exponencial em partial failure, graceful shutdown com flush (US-004)
- `SqsFifoMetadata` extractor expondo `message_group_id`, `message_deduplication_id`, `sequence_number` para subscribers FIFO (US-005)
- `SqsFifoProducer` com `FifoSendBuilder` type-state (`NoGroupId` → `HasGroupId`) — `send()` só compila após `.message_group_id(...)`, eliminando erros runtime de FIFO inválido (US-005)
- `#[subscriber(driver = "sqs", queue = "...", fifo)]` valida em compile-time que o handler declara `SqsFifoMetadata` (US-005)
- `SqsSubscriber` implementa `tower::Service<SqsMessage>` em `serverust-events/src/sqs/subscriber.rs` — pipeline `TracingLayer → MetricsLayer → IdempotencyLayer → RetryLayer → handler` reaproveita `serverust-telemetry` (US-006)
- `IdempotencyLayer` em `serverust-events/src/sqs/layers.rs` (feature `sqs`) — at-least-once → effectively-once com `IdempotencyStore` (in-memory + DynamoDB), protocolo InProgress/Completed + TTL configurável default 24h (US-007)
- `RetryLayer` + `DlqLayer` declarativos via macro `#[subscriber(retry = exponential(max = 5, base = "100ms"), dlq = "orders-dlq")]` — política em metadata, código de negócio limpo (US-008)
- `HeartbeatLayer` em `serverust-events/src/sqs/heartbeat.rs` — `ChangeMessageVisibility` automático em background quando 30% do timeout resta; ativo por default no standalone, opt-in em Lambda ESM (US-009)
- `StandaloneSqsBroker` em `serverust-events/src/sqs/standalone.rs` — long-poll worker para ECS/EC2/bare-metal, concorrência configurável, graceful shutdown drenando in-flight, backoff exponencial em fila vazia. Mesma macro `#[subscriber]` funciona em Lambda ESM e standalone (US-010)
- Transport abstraction — `#[subscriber(driver = "kafka|sqs")]` no mesmo handler; brokers heterogêneos no mesmo app via `EventRouter::attach`; example `examples/transport-swap` (US-011)
- Observability EMF + X-Ray automáticos: métricas `messages_received`, `processing_duration`, `partial_failures`, `dlq_routed`, `idempotency_hits` por queue/handler; span por mensagem com `AWSTraceHeader` propagado outbound pelo producer (US-012)
- Flag opt-in `asyncapi` em `#[subscriber(...)]` — emite método associado `register_asyncapi(builder)` que adiciona `receive` (e `send` se `#[publisher]` empilhado) no `AsyncApiBuilder`; `HAS_ASYNCAPI: bool` exposto (US-013)
- `serverust-events::asyncapi::emit_asyncapi_if_requested(builder, args)` — detecta `--serverust-emit-asyncapi <path>` em `args` e grava spec YAML; integra com `serverust info --asyncapi` (US-013)
- `serverust queue inspect/tail` em `serverust-cli` — lista subscribers/publishers declarados, valida queues + permissões IAM + DLQ stats; saída tabela humana ou `--json` (US-014)

### Changed
- `EventRouter::attach` aceita qualquer `impl Broker` (não só `KafkaBroker`)
- Feature `aws_lambda_events/sqs` ativada pela feature flag `sqs` no `serverust-events`

### Fixed
- Structured logging consistente: substituído `eprintln!` por `tracing::{warn, error}` em `consumer.rs`, `producer.rs`, `layers.rs` — evita vazamento ad-hoc em CloudWatch
- Graceful degradation em `serde_json::to_vec(SqsMessage)` — falha de alocação não causa panic; metadata header omitido e `SqsMetadata` extractor falha com erro claro
- `IdempotencyLayer` agora emite `tracing::warn` quando bypassa por `message_id` vazio (era silencioso)

### Preserved
- `serverust-core` continua sem deps de SQS (invariante CLAUDE.md verificada via `cargo tree`)
- `examples/hello-world` sem dep transitiva de SQS
- Cold start ARM64 128MB < 50ms p95 mantido (feature `sqs` é opt-in)

## [0.2.0] - 2026-05-16

### Added
- Trait `EventHandler<E>` em `serverust-core` paralela ao Router HTTP (US-001)
- Dispatcher multi-trigger em `serverust-lambda` detectando HTTP vs Event automaticamente (US-002)
- Nova crate `serverust-events` com extractor `KafkaRecord<T>` (US-003)
- Macro `#[kafka_consumer(topic, group)]` em `serverust-macros` (US-004)
- `KafkaProducer` injetável atrás de feature `kafka-producer` opt-in (US-005)
- `DynamoRepo<T>` repository pattern + macro `#[dynamo_table]` (US-006)
- Exemplo `examples/kafka-wallet` end-to-end Kafka→Dynamo→Kafka (US-007)
- Baseline competitivo `examples/baselines/axum-raw-kafka` (US-009)
- `CHANGELOG.md` versionado (Keep a Changelog 1.1.0) + gate `quality_changelog.sh` (US-013)
- `CLAUDE.md` na raiz + 5 ADRs MADR em `docs/development/decisions/` (US-015)
- `docs/development/for-ai-agents.md` — guia máquina-legível (US-017)
- `docs/product/metrics/history.json` + schema + scripts `metrics_append.sh`/`metrics_regression_check.sh` (US-014)
- Gate `scripts/quality_kpi_gate.sh` no pre-push (US-016)
- Análise competitiva de `actix-web` em `docs/product/competitors/actix.md` (US-018)
- Entrada v0.2.0 em `release-competitive-log.md` com números reais + baseline (US-010)
- Tabelas competitivas em README + `rocket.md`/`loco.md`/`actix.md` (US-011)
- `docs/development/release-checklist.md` + issue template GitHub com itens `required:true` (US-012)

### Changed
- Renomeado `AGENTS.md` → `CLAUDE.md` (projeto usa Claude Code, não Codex)
- Tabela comparativa do README inclui colunas Rocket / Loco / actix-web

### Preserved (não-regressão)
- `examples/hello-world` mantém SLOs históricos (< 10 MB stripped, < 2000 ms cold start) (US-008)
- Pitch HTTP-first intacto: tudo event-driven em crate opt-in `serverust-events`

## [0.1.2] - 2026-05-16

### Added
- IaC compatibility contract for Serverless Framework, SST and Terraform (`docs/guides/iac-compatibility.md`)
- Release checklist, competitive log and issue template (`docs/product/competitors/release-competitive-log.md`)

### Changed
- Pre-push lefthook hooks scoped to `serverust-core` only (coverage + mutation)
- Quality gates added to pre-commit: lint, complexity, cycle detection, formatting

### Fixed
- CLI scaffold templates now reference crates.io instead of local path
- Friendly CLI message when `cargo-watch` or `cargo-lambda` are missing

## [0.1.1] - 2026-05-14

### Added
- Branding: Ferris 🦀 mascot, startup feedback and first-compilation output

## [0.1.0] - 2026-05-12

### Added
- Cargo workspace with crates: `serverust-core`, `serverust-macros`, `serverust-lambda`, `serverust-cli`, `serverust-telemetry`
- HTTP routing via declarative macros (`#[get]`, `#[post]`, `#[put]`, `#[delete]`)
- App builder and Lambda/HTTP dual-runtime with auto-detection
- Dependency injection via builder pattern
- OpenAPI automatic generation with utoipa + Swagger UI
- Request validation with `#[derive(Validate)]` and standardised error shapes
- Guards, Pipes and Interceptors middleware
- AWS Powertools telemetry: structured logger, tracing and metrics
- CLI (`serverust-cli`): `new`, `generate`, `dev`, `build`, `deploy`, `info`, `openapi` commands
- Configuration via `rustapi.toml` with figment
- Examples: `hello-world`, `funds-api`, `todo-api`
- Essential rustdoc on all public APIs
- MIT OR Apache-2.0 dual license

[Unreleased]: https://github.com/JaimeJunr/serverust/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/JaimeJunr/serverust/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/JaimeJunr/serverust/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/JaimeJunr/serverust/releases/tag/v0.1.0
