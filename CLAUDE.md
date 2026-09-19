# CLAUDE.md — Guia para Claude Code e Maintainers

> Este arquivo é lido automaticamente pelo Claude Code e destina-se a agentes AI e mantenedores humanos.
> Para o guia detalhado de AI agents, veja [`docs/development/for-ai-agents.md`](docs/development/for-ai-agents.md) (criado em US-017).

---

## Filosofia (o critério quando houver trade-off)

Contexto completo em [`docs/product/philosophy.md`](docs/product/philosophy.md). Resumo operacional — use isto para decidir, não só para entender:

1. **Segurança vem da linguagem.** Prefira erro de compilação a verificação em runtime. Rotas, schema OpenAPI e providers são resolvidos em compile-time; mantenha assim.
2. **Baixo nível sem escrever baixo nível.** Custo em Lambda é memória × tempo — por isso cold start e tamanho de binário são invariantes com gate, não aspirações (tabela abaixo).
3. **DX é requisito, não enfeite.** O usuário do framework deve gastar o tempo dele no domínio do problema. Boilerplate que o framework poderia absorver é bug de design.

Como isso resolve os conflitos mais comuns:

| Situação | Decisão |
|---|---|
| Feature útil que pesa no cold start ou no binário | Feature flag opt-in — nunca default em `serverust-core` |
| Performance × DX | Resolva em compile-time (macro ou builder); não empurre a complexidade pro usuário |
| Macro nova | Permitida, mas exige builder programático equivalente por baixo |
| Abstração que esconderia o Axum | Rejeitada — `App::axum_router()` é escape hatch de primeira classe |
| Default com efeito de segurança | Falha fechado; a exceção é anotação explícita, nunca omissão (ver abaixo) |
| Regressão de invariante público | Exige ADR aprovada antes do merge |

**Defaults na era dos agentes.** Boa parte do código deste projeto — e dos projetos que o usam — é escrita por agentes de IA, que **falham por omissão, não por comissão**. Daí três regras ao desenhar qualquer default:

1. **Falhe fechado.** Sob default permissivo a omissão é invisível (compila, passa nos testes, diff limpo). Sob default restritivo ela quebra o caminho feliz na hora — um buraco silencioso vira bug barulhento, e bug barulhento é corrigido.
2. **Torne a exceção auditável por presença.** Marque o que é perigoso (`#[public]`), não o que é seguro: `grep` encontra presença, nunca ausência.
3. **Não dependa de lembrar.** Garantia imposta por tipo, compilador ou default restritivo custa o mesmo no primeiro e no milésimo endpoint; disciplina não.

Contexto completo em [`philosophy.md`](docs/product/philosophy.md#o-corolário-defaults-na-era-dos-agentes); primeira aplicação na [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md).

**Ao divulgar performance** (README, docs, release notes): publique a ressalva junto com o número. O caso de produção documentado rende "8x" se medido só pelo `Init Duration`, e **~2x** no caminho frio completo — o número honesto é o que vale. Nunca cite `Init Duration` isolado como ganho de cold start.

---

## Invariantes (SLOs públicos — não negociáveis sem ADR)

Estas propriedades são compromissos públicos. Violá-las exige uma nova ADR aprovada em `docs/development/decisions/`.

| Invariante | Limite | Medição |
|---|---|---|
| Cold start ARM64 128 MB (`hello-world`) | **< 50 ms** p95 | sem medição automatizada — exige `cargo-lambda` + invocação real na AWS (ver ADR 0008) |
| Binário stripped (`hello-world`) | **< 10 MB** | `scripts/benchmark_ci.sh` |
| `serverust-core` sem deps de eventos/Kafka | zero | `cargo tree -p serverust-core \| grep -E "kafka\|rdkafka\|event"` |
| `hello-world` sem deps de Kafka/DynamoDB | zero | `cargo tree -p hello-world \| grep -v -e kafka -e dynamo` |
| Typecheck do workspace | verde | `cargo check --workspace` |
| Métricas da versão corrente preenchidas | `stripped_bytes` + `startup_local_p50_ms` não-null em `history.json` | `scripts/quality_metrics_required.sh` (gate obrigatório no pre-push) |
| Startup local (`hello-world`) | **informativo** — só reprova acima de 2000 ms | `scripts/quality_kpi_gate.sh` (ver ADR 0008) |

**Regressão detectada?** Crie uma ADR em `docs/development/decisions/` justificando antes de mergear.

---

## Processo de Release

A partir de v0.4: **per-crate independent versioning** (estilo tokio/axum). Tag por crate `<crate-name>-vX.Y.Z`. Pré-v0.4 usava workspace-wide unified versioning.

### Fluxo recomendado (release-plz, automatizado via CI)

[`release-plz`](https://release-plz.dev) é Rust-native, dispara automaticamente:

1. Merge commits seguindo Conventional Commits (`feat:`, `fix:`, `chore:`) no `main`.
2. release-plz abre Release PR com bump per-crate + CHANGELOG via git-cliff + cargo-semver-checks.
3. Merge do Release PR → `cargo publish` (ordem certa) + git tags `<crate>-v<X.Y.Z>` + GitHub Release.

Configs:
- `release-plz.toml` — quais crates publicar, política de tags.
- `cliff.toml` — template CHANGELOG.
- `cog.toml` — Conventional Commits via cocogitto.
- `.github/workflows/release-plz.yml` — CI workflow.

Pré-flight: secret `CARGO_REGISTRY_TOKEN` (gere em https://crates.io/me).

Trigger manual: Actions → release-plz → "Run workflow".

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
serverust-macros-tests/   # Integração das macros (trybuild + runtime); evita ciclo de dev-deps macros↔core/events/telemetry
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
scripts/quality_cycles.sh     # dependências cíclicas (cargo-cycles)
scripts/quality_machete.sh    # dependências declaradas e não usadas
```

### Ferramenta ausente ≠ gate verde

Aplicação direta de "falhe fechado" aos próprios gates: **a ausência de verificação nunca pode ser indistinguível de verificação bem-sucedida.** Todo gate que depende de ferramenta externa distingue três estados, via `require_tool` em [`scripts/lib/tool_guard.sh`](scripts/lib/tool_guard.sh):

| Estado | Comportamento |
|---|---|
| Ferramenta presente, checagem passou | exit 0, silencioso |
| Ferramenta presente, checagem falhou | exit ≠ 0 — bloqueia |
| Ferramenta **ausente** | aviso alto em stderr (`PULADO: ... NENHUMA verificação`), exit ≠ 0 **em CI** (`$CI`), exit 0 localmente |

O desenvolvedor local não trava por falta de ferramenta; o CI nunca reporta verde sem ter verificado. `quality_coverage.sh` e `quality_mutation.sh` são mais estritos — reprovam na ausência inclusive localmente, porque a instalação é pré-requisito de push.

Ao adicionar um gate novo com dependência externa: use `require_tool`, nunca `if command -v ...; then ...; fi` com `else` silencioso. [`scripts/test_lefthook_hooks.sh`](scripts/test_lefthook_hooks.sh) (pre-commit) guarda os três estados extraindo os `run:` reais do `lefthook.yml`.

No CI (`.github/workflows/lint.yml`) os gates espelhados **instalam a ferramenta em passo explícito** do job — `fmt`, `clippy` e `cycles` hoje. Se a instalação falhar, o guard reprova em vez de passar vazio: verde ali significa que a checagem rodou.

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
