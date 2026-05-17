# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- MAINTENANCE: When bumping workspace.version in Cargo.toml, add a new ## [x.y.z] section
     above [Unreleased] with date YYYY-MM-DD and move relevant [Unreleased] entries there. -->

## [Unreleased]

### Added
- `serverust-events::sqs::extract::SqsFifoMetadata` extractor expondo `message_group_id`, `message_deduplication_id` e `sequence_number` para subscribers FIFO (US-005)
- `serverust-events::sqs::fifo_producer::SqsFifoProducer` com `FifoSendBuilder` type-state (`NoGroupId` → `HasGroupId`): `send()` só compila após `.message_group_id(...)`, evitando publicação FIFO inválida em runtime (US-005)
- Macro `#[subscriber(driver = "sqs", queue = "...", fifo)]` valida em compile-time que o handler declara `SqsFifoMetadata`; uso indevido em queue standard ou FIFO sem o flag emite erro de compilação claro (US-005)
- `SendEntry::message_group_id` e `SendEntry::message_deduplication_id` propagados pelo `SqsProducer` para entregas FIFO (US-005)
- Trait `Broker` (`subscribe` + `publish` assíncronos) e tipos `BrokerMessage`, `BrokerError`, `BoxedHandler` em `serverust-events/src/broker/mod.rs` (US-1 do workspace `serverust-events`)
- `KafkaBroker` (rust-rdkafka) atrás da nova feature `kafka` — pavimenta o EventRouter (US-3) sem acoplar `serverust-core` ao Kafka
- `InMemoryBroker` em `serverust-events/src/broker/in_memory.rs` (feature `in-memory`) — entrega síncrona em memória para testes sem broker físico (US-2)
- `EventRouter` builder programático em `serverust-events/src/router.rs`: `subscribe::<T, _>(topic, handler)` com decodificação JSON, `with_retry(RetryPolicy)`, `with_dlq(topic)`, `attach(&broker)` aceitando qualquer `impl Broker` (US-3)
- `RetryPolicy` (variants `Immediate` / `Exponential`) em `serverust-events/src/retry.rs` — tipo público consumido pelo `EventRouter::with_retry`; aplicação runtime fica em US-5
- `EventRouter::subscribe_publish::<T, U, _, _>(sub_topic, pub_topic, handler)` em `serverust-events/src/router.rs` — registra handler que serializa `Ok(U)` e publica em `pub_topic` (US-6)
- Macros `#[subscriber(topic = "...")]` e `#[publisher(topic = "...")]` empilháveis em `serverust-macros/src/lib.rs` — emitem código baseado no builder `EventRouter::subscribe` / `subscribe_publish`, sem registro runtime (US-6)
- `Runtime::detect()` em `serverust-events/src/runtime.rs` — diferencia execução em AWS Lambda (`AWS_LAMBDA_FUNCTION_NAME` presente) de processos long-running (ECS/EC2) sem acoplar o EventRouter ao adapter (US-7)
- `LambdaBroker` em `serverust-events/src/broker/lambda.rs` — broker sink-only que despacha `aws_lambda_events::KafkaEvent` para handlers inscritos via `handle_kafka_event`. Independente da feature `kafka` (não puxa rdkafka), pronto para uso direto em Lambda Functions (US-7)
- `KafkaBroker::dispatch(BrokerMessage)` em `serverust-events/src/broker/kafka.rs` — primitiva consumida pelo consumer loop long-running, testada de forma isolada sem broker físico (US-7)
- Módulo `asyncapi` em `serverust-events/src/asyncapi.rs`: `AsyncApiBuilder` que gera spec [AsyncAPI 3.0](https://www.asyncapi.com/docs/reference/specification/v3.0.0) com `channels` (tópicos), `operations` (`receive`/`send`) e `components` (messages + JSON Schema embarcado via `schemars`); `AsyncApiSpec::to_yaml()` serializa YAML válido (US-8)
- Subcomando `serverust info --asyncapi [--out path]` em `serverust-cli` — spawna `cargo run -- --serverust-emit-asyncapi <out>` para extrair o spec do binário do projeto sem subir consumer/producer (US-8)

### Changed
- Feature `kafka-producer` agora é alias de `kafka` — sem mudança de comportamento para usuários existentes

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
