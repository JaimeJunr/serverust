# CLAUDE.md — Guia para Claude Code e Maintainers

> Este arquivo é lido automaticamente pelo Claude Code e destina-se a agentes AI e mantenedores humanos.
> Para o guia detalhado de AI agents, veja [`docs/development/for-ai-agents.md`](docs/development/for-ai-agents.md) (criado em US-017).

---

## Invariantes (SLOs públicos — não negociáveis sem ADR)

Estas propriedades são compromissos públicos. Violá-las exige uma nova ADR aprovada em `docs/development/decisions/`.

| Invariante | Limite | Medição |
|---|---|---|
| Cold start ARM64 128 MB (`hello-world`) | **< 50 ms** p95 | `scripts/benchmark_ci.sh --lambda` |
| Binário stripped (`hello-world`) | **< 10 MB** | `scripts/benchmark_ci.sh` |
| `serverust-core` sem deps de eventos/Kafka | zero | `cargo tree -p serverust-core \| grep -E "kafka\|rdkafka\|event"` |
| `hello-world` sem deps de Kafka/DynamoDB | zero | `cargo tree -p hello-world \| grep -v -e kafka -e dynamo` |
| Typecheck do workspace | verde | `cargo check --workspace` |
| Métricas da versão corrente preenchidas | `stripped_bytes` + `cold_start_p95_ms` não-null em `history.json` | `scripts/quality_metrics_required.sh` (gate obrigatório no pre-push) |

**Regressão detectada?** Crie uma ADR em `docs/development/decisions/` justificando antes de mergear.

---

## Processo de Release

A partir de v0.4: **per-crate independent versioning** (estilo tokio/axum). Tag por crate `<crate-name>-vX.Y.Z`. Pré-v0.4 usava workspace-wide unified versioning.

### Fluxo recomendado (cargo-release)

```bash
# Patch release de um crate específico:
cargo release patch -p serverust-events --execute

# Workspace inteiro (bump + tag + publish na ordem certa, tudo atômico):
cargo release patch --workspace --execute
```

`cargo-release` (config em `release.toml`) faz: bump → CHANGELOG date → commit → tag SSH-signed → push → `cargo publish` na ordem de dependência.

### Fluxo manual (alternativa)

1. Incrementar `version` no(s) `Cargo.toml` do(s) crate(s) afetado(s).
2. Atualizar refs path-deps internas (`version = "X.Y.Z"`).
3. Mover items de `[Unreleased]` para nova versão com data no `CHANGELOG.md`.
4. Rodar `scripts/quality_changelog.sh` — deve passar.
5. Rodar `scripts/benchmark_ci.sh` + `scripts/metrics_append.sh <version>`.
6. Rodar `scripts/benchmark_competitive.sh` (atualiza `release-competitive-log.md`).
7. Criar tag git assinada: `git tag -s <crate>-v<VERSION> -m "Release <crate> v<VERSION>"` (per-crate) ou `git tag -s v<VERSION>` (workspace).
8. Publicar (Cargo 1.90+): `cargo publish --workspace` resolve ordem. Ou sequencial: `serverust-macros` → `serverust-core` → `serverust-telemetry` → `serverust-events` → `serverust-lambda` → `serverust-cli`.
9. Confirmar que CI passou **com a tag** (histórico: v0.1.1 e v0.1.2 foram publicadas sem tag — não repetir).

### Pré-flight obrigatório

- SSH signing configurado (`gpg.format = ssh`, `user.signingkey = ~/.ssh/id_*.pub`, `tag.gpgsign = true`).
- Public key adicionada como **Signing key** em GitHub settings.
- `cargo login` configurado.
- `cargo deny check` verde (CI: `.github/workflows/cargo-deny.yml`).

Referência canônica: [`docs/development/release-checklist.md`](docs/development/release-checklist.md).

---

## Estrutura do Workspace

```
serverust-core/           # App builder, Route, DI Container, pipeline, OpenAPI
serverust-macros/         # Proc-macros: #[get], #[post], #[injectable], #[guard], ...
serverust-lambda/         # Adapter Lambda: AppRuntime, detect_runtime, run_lambda()
serverust-telemetry/      # Logger JSON, tracing X-Ray, métricas EMF, IdempotencyStore
serverust-cli/            # CLI: new/generate/dev/build/deploy/info/openapi
serverust-events/         # (v0.2.0) Event-driven opt-in: KafkaRecord, KafkaProducer
examples/hello-world/     # Benchmark de cold start — NÃO adicionar deps extras aqui
examples/funds-api/       # CRUD completo com DI e testes de integração
examples/kafka-wallet/    # (v0.2.0) Exemplo Kafka→Dynamo→Kafka end-to-end
examples/baselines/       # Implementações vanilla para benchmark competitivo
docs/                     # Documentação pública e de desenvolvimento
scripts/                  # Shell scripts de qualidade e benchmark
```

### Onde colocar código novo

| Tipo de código | Onde vai |
|---|---|
| Nova feature HTTP | `serverust-core` ou `serverust-macros` |
| Adapter Lambda novo trigger | `serverust-lambda` |
| Feature Kafka / event-driven | `serverust-events` (NUNCA em `serverust-core`) |
| Feature DynamoDB | `serverust-telemetry` atrás de `feature = "dynamodb"` |
| Feature rdkafka producer | `serverust-events` atrás de `feature = "kafka-producer"` |
| Exemplo de uso | `examples/<nome>/` |
| Baseline competitivo | `examples/baselines/<nome>/` com `publish = false` |
| ADR nova | `docs/development/decisions/XXXX-titulo.md` (formato MADR 4.0) |

---

## Quality Gates Obrigatórios

### Pre-commit (automático via lefthook)

```bash
scripts/quality_fmt.sh        # rustfmt check
scripts/quality_lint.sh       # clippy --deny warnings
scripts/quality_complexity.sh # complexidade ciclomática
scripts/quality_cycles.sh     # dependências cíclicas
```

### Pre-push (automático via lefthook)

```bash
scripts/quality_changelog.sh  # CHANGELOG.md atualizado se versão mudou
scripts/quality_coverage.sh   # cobertura mínima nos crates core
scripts/quality_mutation.sh   # mutation testing nos crates core
```

### Opt-in local / obrigatório em CI de release

```bash
LEFTHOOK_KPI=1 git push       # gate de KPI: binário e cold start vs history.json
```

Rodar manualmente: `scripts/quality_kpi_gate.sh`

**Override de emergência**: `LEFTHOOK_KPI_SKIP=1 LEFTHOOK_KPI=1 git push`
Quando usado, a justificativa **deve** constar na mensagem do commit (ex: `reason: regressão aceitável — aguardando nova release de rdkafka`).
Sem justificativa no commit, o PR não será aprovado.

---

## Antes de Mergear (Checklist)

- [ ] `cargo check --workspace` passa sem warnings
- [ ] `cargo test -p <crates-afetados>` passa
- [ ] Invariantes da tabela acima continuam válidas
- [ ] Se mudou `Cargo.toml` version → `CHANGELOG.md` tem entrada para a versão
- [ ] Se adicionou dep em `serverust-core` → justificativa em ADR ou PR description
- [ ] Se tocou em `examples/hello-world` → rodar `scripts/benchmark_ci.sh` e confirmar limites
- [ ] Se a mudança exige novo comportamento documentado → ADR criada em `docs/development/decisions/`
- [ ] Tag git criada se for release (veja Processo de Release acima)

---

## Referências

- [Guia para AI Agents](docs/development/for-ai-agents.md)
- [ADRs](docs/development/decisions/)
- [CHANGELOG](CHANGELOG.md)
- [Release Checklist](docs/development/release-checklist.md)
- [Documentação completa](docs/INDEX.md)
