# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- MAINTENANCE: When bumping workspace.version in Cargo.toml, add a new ## [x.y.z] section
     above [Unreleased] with date YYYY-MM-DD and move relevant [Unreleased] entries there. -->

## [Unreleased]

### Added
- Event system roadmap (US-001–US-009): EventHandler trait, Kafka extractor, macro `#[kafka_consumer]`, KafkaProducer opt-in feature, DynamoRepo pattern
- CLAUDE.md and ADRs in MADR format under `docs/development/decisions/`
- `docs/development/for-ai-agents.md` machine-readable guide
- `docs/product/metrics/history.json` versioned KPI history
- Quality gates: `scripts/quality_kpi_gate.sh`, `scripts/quality_changelog.sh`

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
