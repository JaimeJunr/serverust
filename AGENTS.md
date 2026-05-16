# AGENTS.md — Guia para AI Agents e Maintainers

> Este arquivo é máquina-legível e destina-se a agentes AI e mantenedores humanos.
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

**Regressão detectada?** Crie uma ADR em `docs/development/decisions/` justificando antes de mergear.

---

## Processo de Release

1. Incrementar versão em `Cargo.toml` (workspace `version`).
2. Atualizar `CHANGELOG.md` — mover items de `[Unreleased]` para a nova versão com data.
3. Rodar `scripts/quality_changelog.sh` — deve passar.
4. Rodar `scripts/benchmark_ci.sh` — registrar saída em `docs/product/metrics/history.json`.
5. Rodar `scripts/benchmark_competitive.sh` — atualizar `docs/product/competitors/release-competitive-log.md`.
6. Criar tag git: `git tag -s v<VERSION> -m "Release v<VERSION>"`.
7. Publicar crates na ordem: `serverust-macros` → `serverust-core` → `serverust-telemetry` → `serverust-lambda` → `serverust-cli`.
8. Confirmar que CI passou **com a tag** (histórico: v0.1.1 e v0.1.2 foram publicadas sem tag — não repetir).

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
