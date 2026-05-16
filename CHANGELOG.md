# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- MAINTENANCE: When bumping workspace.version in Cargo.toml, add a new ## [x.y.z] section
     above [Unreleased] with date YYYY-MM-DD and move relevant [Unreleased] entries there. -->

## [Unreleased]

### Added
- Trait `Broker` (`subscribe` + `publish` assíncronos) e tipos `BrokerMessage`, `BrokerError`, `BoxedHandler` em `serverust-events/src/broker/mod.rs` (US-1 do workspace `serverust-events`)
- `KafkaBroker` (rust-rdkafka) atrás da nova feature `kafka` — pavimenta o EventRouter (US-3) sem acoplar `serverust-core` ao Kafka

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
