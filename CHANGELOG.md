# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- MAINTENANCE: When bumping workspace.version in Cargo.toml, add a new ## [x.y.z] section
     above [Unreleased] with date YYYY-MM-DD and move relevant [Unreleased] entries there. -->

## Week ending 2026-05-31

> Digest da auditoria semanal (commits `92cdd3a`..`c6c35ab`). Itens de produto acumulam em `[Unreleased]` abaixo.

### Fixed

- `EventRouter` com `RetryPolicy::Exponential`: backoff com `Duration::saturating_mul` e expoente limitado a 31 — sem panic por overflow (#13).

### Changed

- `serverust-cli`: `version.workspace = true` em `Cargo.toml`, alinhado aos demais crates publicáveis (#19).

### Documentation

- SQS v0.3: guia [event-driven.md](docs/guides/event-driven.md), roadmap v0.3 entregue, `INDEX.md` e overview (#16).
- Sync semanal: comportamento de backoff documentado no guia; patches pós-0.3.0 no roadmap (#17).

## [Unreleased]

### Documentation

- [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md) (Accepted): auth/authz em crate separada `serverust-auth` — validação de JWT de IdP externo, RBAC/scopes em compile-time, JWKS aquecido na fase de init por construção da API, cripto em Rust puro e **default deny** (rota sem anotação é negada; `#[public]` é a exceção explícita).
- Filosofia: novo corolário [defaults na era dos agentes](docs/product/philosophy.md) — falhe fechado, torne a exceção auditável por presença e não dependa de lembrar. Regra espelhada no `CLAUDE.md`.

- Filosofia do projeto em [`docs/product/philosophy.md`](docs/product/philosophy.md): segurança pela linguagem, baixo nível sem escrever baixo nível e DX como requisito — com o caso real de migração NestJS → serverust em produção (Lambda ARM64), os números medidos e a ressalva de como lê-los (#40). Resumo no `README.md`, critério de trade-off e regra de divulgação de performance no `CLAUDE.md`, links em `INDEX.md` e `vision.md`.

## [0.4.2] - 2026-09-14

Duas adições à API pública de `serverust-core`, ambas **compatíveis** — nada muda para quem já usa a 0.4.x. O restante é infraestrutura do repositório e não alcança quem consome os crates.

Versão de patch, não minor, apesar de `feat`: em `0.x` o Cargo trata `0.5.0` como incompatível com `0.4.x`, e uma 0.5.0 exigiria que cada consumidor editasse o `Cargo.toml` para receber uma mudança que é puramente aditiva. Com `0.4.2`, quem declara `serverust-core = "0.4"` recebe automaticamente.

### Added

- `serverust-core`: `App::layer(...)` para aplicar qualquer `tower::Layer` genérico (ex.: `axum::extract::DefaultBodyLimit`, CORS, timeout, compressão) sobre as rotas do usuário, sem precisar implementar a trait `Interceptor` nem espelhar o `run()` do framework à mão (#39).
- `serverust-core`: `App::without_docs()` desabilita o registro de `/openapi.json`, `/docs` e `/redoc` em `into_router()`, para serviços internos que não querem expor essa superfície (#39).

### Changed

- `scripts/quality_kpi_gate.sh`: o eixo de startup local deixa de reprovar por comparação com o baseline e passa a informativo, com guarda absoluta em 2000 ms. Medições isoladas da mesma build, sem alteração de código, variaram de 11 ms a 62 ms conforme a carga da máquina — a tolerância de 20% sobre um baseline de 11 ms reprovava ruído. `stripped_bytes` continua gate estrito em 5%. Ver ADR 0008.
- `scripts/benchmark_ci.sh` e `scripts/metrics_append.sh`: o startup passa a ser a mediana de 5 medições (`STARTUP_SAMPLES`) em vez de uma amostra única, que deixava o baseline refém de onde caiu na distribuição.
- `docs/product/metrics/history.json`: o campo medido passa a se chamar `startup_local_p50_ms`, acompanhado de `startup_local_samples`. O que o script mede é o tempo até a primeira resposta HTTP de um binário local — não cold start de Lambda. `cold_start_p95_ms` permanece no schema como o campo do invariante público e fica `null` enquanto não houver invocação real na AWS. Leitores aceitam o nome antigo como fallback.

## [0.4.1] - 2026-09-13

Release de manutenção: atualiza duas dependências com advisory do RUSTSEC no `Cargo.lock`. **Sem mudança de API** — quem usa os crates como biblioteca pode pular.

**Quem deve atualizar:** quem instala o CLI com `cargo install serverust-cli --locked`, já que esse fluxo usa o `Cargo.lock` publicado no pacote. Quem depende dos crates como biblioteca resolve as próprias dependências e já pegava as versões corrigidas, por serem semver-compatíveis.

### Changed

- `serverust-events`: `handle_sqs_event` e `flush` decompostos em funções menores (complexidade cognitiva 32→6 e 27→3). Refactor interno, sem mudança de API nem de comportamento — os testes existentes passaram sem alteração.

### Security

- `Cargo.lock`: `h2` 0.4.14 → 0.4.19 (RUSTSEC-2026-0258, DoS por DATA frames vazios sem limite) e `anyhow` 1.0.102 → 1.0.104 (RUSTSEC-2026-0190, unsoundness em `Error::downcast_mut()`). Consumidores das bibliotecas não eram afetados: o `Cargo.lock` não participa da resolução de dependências de quem depende dos crates, e ambos os fixes são semver-compatíveis. O impacto real era em quem builda este repositório e em quem instala o CLI com `cargo install serverust-cli --locked`, fluxo que usa o `Cargo.lock` publicado no pacote.
- `deny.toml`: dois advisories sem ação possível deste lado passam a ter ignore documentado, em vez de deixar `cargo deny` vermelho mascarando achado novo — `h2` 0.3.27 (sem patch na linha 0.3.x, chega só pelo cliente HTTP do AWS SDK) e `proc-macro-error2` (unmaintained, proc-macro de build-time via `validator_derive`).

## [0.4.0] - 2026-09-13

`serverust-telemetry 0.4.0` traz uma **quebra de API** no `IdempotencyStore` e dois fixes que **mudam o comportamento em runtime sem quebrar a compilação** — leia "Migração" antes de subir em produção.

### Migração desde 0.3.x

**1. `IdempotencyStore` agora exige token de fencing.** Só afeta quem implementa o trait ou faz `match` em `AcquireOutcome` diretamente; quem só monta o `IdempotencyLayer` com `InMemoryIdempotencyStore` / `DynamoDbIdempotencyStore` não precisa mudar nada.

```rust
// antes
match store.try_acquire(&key, now, ttl).await? {
    AcquireOutcome::Acquired => { /* ... */ store.complete(&key, now, ttl).await?; }
    // ...
}

// depois — o token identifica o dono do lock
match store.try_acquire(&key, now, ttl).await? {
    AcquireOutcome::Acquired(token) => { /* ... */ store.complete(&key, &token, now, ttl).await?; }
    // ...
}
```

Quem implementa o trait: `release`/`complete` devem virar no-op de sucesso quando o token não bate com o do registro corrente — é o que impede um worker cujo TTL expirou de apagar o lock de outro. Na tabela DynamoDB isso vira uma `condition_expression` com estado **e** token; nenhuma migração de schema é necessária (o atributo `token` passa a ser gravado nos registros novos).

**2. Mudanças de comportamento em produção** — nada a alterar no código, mas o sistema passa a agir diferente:

| Cenário | 0.3.x | 0.4.0 |
|---|---|---|
| Handler falha com `IdempotencyLayer` ativo | Lock `InProgress` ficava até o TTL (24h por padrão); redeliveries do SQS não reexecutavam o handler e a mensagem ia para a DLQ sem nunca ser processada | Lock é liberado; a próxima redelivery reexecuta o handler |
| `EventRouter::with_dlq`, publish na DLQ bem-sucedido | Retornava `Err`, a Lambda não removia a mensagem da fila — loop de redelivery com escrita duplicada na DLQ | Retorna `Ok(())`, a mensagem original recebe ack (mesma semântica do `DlqLayer`) |

O efeito prático do primeiro é **mais reprocessamento** de mensagens que antes ficavam presas: se o handler não for idempotente por conta própria além do lock, verifique isso antes de subir. O do segundo é **menos escrita duplicada na DLQ**.

**3. `tracing` virou dependência não-opcional de `serverust-events`.** Antes vinha só com a feature `sqs`. Nenhuma ação necessária — apenas note o acréscimo na árvore de dependências se você audita footprint.

**4. Tópico Kafka sem handler continua sendo ignorado** (agora com `tracing::warn!` em vez de silêncio). Se preferir que isso falhe, é opt-in: `.with_unhandled_topic_policy(UnhandledTopicPolicy::Error)`.

### Added

- `UnhandledTopicPolicy` em `LambdaBroker` e `KafkaBroker` (`with_unhandled_topic_policy`): `WarnAndIgnore` (default) preserva o comportamento 0.3.x de pular o record sem handler e passa a emitir `tracing::warn!` com o tópico; `Error` retorna `BrokerError::Subscribe` com o tópico recebido e a lista de tópicos inscritos.

### Changed

- `serverust-events`: `tracing` deixa de ser dependência opcional (antes só sob a feature `sqs`) — `UnhandledTopicPolicy` e o log de falha da DLQ em `EventRouter` rodam em código sem a feature `sqs`.
- CI e desenvolvimento local passam a usar toolchain Rust pinada em `rust-toolchain.toml` (1.94.1) em vez de `stable` flutuante — os testes `trybuild` de `serverust-macros` comparam a saída literal do rustc e quebravam a cada mudança de formatação de diagnóstico.
- `serverust-cli`: passa a usar `version.workspace = true` em `Cargo.toml`, herdando `workspace.package.version` como os demais crates publicáveis (evita drift de versão do binário `serverust`).
- **BREAKING** (`serverust-telemetry`): `IdempotencyStore::try_acquire` devolve `AcquireOutcome::Acquired(LockToken)` em vez de `Acquired`, e `release`/`complete` passam a exigir o token da aquisição (fencing). Token divergente é no-op de sucesso. Implementações externas de `IdempotencyStore` e qualquer `match` sobre `AcquireOutcome::Acquired` precisam ser ajustados.

### Fixed

- Hooks `machete` e `cog-verify` no lefthook deixavam de bloquear quando a ferramenta existia e reprovava (deps não usadas / mensagem de commit inválida); só a ausência da ferramenta deve ser tolerada.
- `EventRouter` com `RetryPolicy::Exponential`: o atraso `base_delay * 2^n` passa a usar `Duration::saturating_mul` e expoente limitado a 31, evitando panic por overflow de `Duration` em retentativas longas ou `base_delay` grande.
- `SqsBroker::handle_sqs_event` (Lambda ESM + `ReportBatchItemFailures`): mensagens sem handler para a fila do ARN ou sem `event_source_arn` válido passam a entrar em `batchItemFailures` quando há `messageId`, em vez de serem tratadas como sucesso implícito (a Lambda removia da fila sem processamento).
- `IdempotencyLayer`: após falha do handler ou de `complete()`, libera o lock `InProgress` via `IdempotencyStore::release`, permitindo que redeliveries do SQS reexecutem o handler dentro do TTL (antes o lock bloqueava reprocessamento por até 24h e a mensagem ia para DLQ sem nova tentativa). `release`/`complete` só mutam o registro se o token bater com o dono corrente — um owner cujo TTL expirou não apaga nem completa o lock de outro worker. Falha de `release` é logada com `tracing::warn` (chave + erro), sem mascarar o erro do handler. Falha de `complete` após sucesso do handler propaga erro ao SQS e libera o lock.
- `EventRouter::with_dlq`: após publicação bem-sucedida no tópico DLQ, o wrapper retorna `Ok(())` (mesma semântica de `DlqLayer`), permitindo ack da mensagem original no Lambda SQS em vez de loop infinito de redelivery. Se o publish na DLQ falhar, o erro original do handler é retornado e ambos os erros (handler e DLQ) são registrados com `tracing::error!`.

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
