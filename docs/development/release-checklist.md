# Release Checklist — serverust

Checklist obrigatório para toda release. Referência canônica linkada em CLAUDE.md.

**Versão única do workspace**: todos os crates `serverust-*` herdam `[workspace.package].version` (`version.workspace = true`) e a release gera uma tag só, `vX.Y.Z`. Per-crate foi planejado para a v0.4, mas nunca adotado — v0.4.0, v0.4.1 e v0.4.2 saíram com tag única.

---

## Antes do Release

- [ ] `cargo check --workspace` passa sem warnings
- [ ] `cargo test -p serverust-core` passa
- [ ] `cargo test -p serverust-macros` passa
- [ ] `cargo test -p serverust-telemetry` passa
- [ ] `cargo test -p serverust-events` passa (com features `sqs in-memory` para cobertura completa)
- [ ] `cargo test -p serverust-lambda` passa
- [ ] `scripts/quality_fmt.sh` passa
- [ ] `scripts/quality_lint.sh` passa
- [ ] `scripts/quality_complexity.sh` passa
- [ ] `scripts/quality_cycles.sh` passa
- [ ] `scripts/quality_changelog.sh` passa (versão em Cargo.toml tem entrada em CHANGELOG.md)
- [ ] `scripts/quality_hello_world.sh` passa (hello-world sem deps Kafka/DynamoDB/SQS)
- [ ] `cargo deny check` passa (CI: `.github/workflows/cargo-deny.yml`)

## Versão e Changelog

### Decisão: workspace-wide vs per-crate

- **Workspace-wide** (o que o projeto usa): bump `workspace.package.version` em `Cargo.toml`. Tag `v0.4.2`. É o que o `release-plz.toml` faz (`version_group = "workspace"`).
- **Per-crate** (não adotado): exigiria tirar o `version.workspace = true` dos crates e voltar a tag para `<crate>-vX.Y.Z` no `release-plz.toml`.

### Etapas

- [ ] Incrementar `version` no(s) `Cargo.toml` do(s) crate(s) afetado(s). Crates publicáveis (`serverust-*`) usam `version.workspace = true` — em release workspace-wide (pré-v0.4), basta bump em `[workspace.package]` no `Cargo.toml` raiz.
- [ ] Atualizar refs path-deps internas para a nova versão (ex: `serverust-events` depende de `serverust-telemetry = "X.Y.Z"`)
- [ ] Mover itens de `[Unreleased]` para nova versão com data em `CHANGELOG.md`
- [ ] `scripts/quality_changelog.sh` verde

## Benchmarks e Métricas

- [ ] `scripts/benchmark_ci.sh` executado — binário stripped hello-world < 10 MB, cold start < 2000 ms
- [ ] Resultado registrado em `docs/product/metrics/history.json` via `scripts/metrics_append.sh <version>`
- [ ] `scripts/metrics_regression_check.sh` verde (sem regressão > 5% em stripped_bytes ou > 10% em cold_start)

## Event Sources

- [ ] `examples/kafka-wallet` compila: `cargo build -p kafka-wallet`
- [ ] Testes do kafka-wallet passam: `cargo test -p kafka-wallet --test dto`
- [ ] `scripts/benchmark_competitive.sh` executado — LOC e métricas serverust vs baseline atualizados
- [ ] `docs/product/competitors/release-competitive-log.md` atualizado com entrada para a nova versão

## Competitivo

- [ ] Versões de Rocket/Loco/actix-web/axum re-validadas nas releases oficiais
- [ ] Tabela comparativa do README atualizada se necessário
- [ ] `docs/product/competitors/rocket.md`, `loco.md`, `actix.md` com seção atualizada

## Publicação

### Opção A — `release-plz` (recomendado, automatizado via CI)

[`release-plz`](https://release-plz.dev) é Rust-native (inspirado pelo Google `release-please` mas otimizado pra Rust). Roda automaticamente em CI:

1. **Você merge commits** no `main` seguindo Conventional Commits (`feat:`, `fix:`, `chore:`).
2. **release-plz abre um Release PR** com o bump da versão do workspace (baseado nos commits) + breaking changes detectadas via cargo-semver-checks. O CHANGELOG **não** é gerado (`changelog_update = false`): mova o `[Unreleased]` para a nova versão num commit na própria Release PR.
3. **Você mergea o Release PR** → release-plz dispara `cargo publish` (na ordem certa) + cria a tag `vX.Y.Z` + abre GitHub Release. Sem Release PR mergeada, o job `release` não publica nada (`release_always = false`).

Configuração:
- `release-plz.toml` na raiz — quais crates publicar, política de tags.
- `cliff.toml` na raiz — template do CHANGELOG.
- `.github/workflows/release-plz.yml` — CI workflow.
- Secret `CARGO_REGISTRY_TOKEN` no environment GitHub (gere em https://crates.io/me).
- Secret `RELEASE_PLZ_TOKEN`: PAT fine-grained só deste repo, com Contents e Pull requests em read/write. Com o `GITHUB_TOKEN` o GitHub recusa abrir a PR (403) e os checks exigidos do `main` não rodariam nela; o workflow reprova se o secret faltar.

Trigger manual: vá em Actions → release-plz → "Run workflow".

### Opção B — `cargo publish --workspace` (manual, Cargo 1.90+)

A partir de Cargo 1.90 (Nov 2025) o `cargo publish` aceita `--workspace` e resolve ordem de dependência sozinho:

```bash
cargo publish --workspace            # publica TODOS os crates do workspace
cargo publish -p serverust-macros -p serverust-core   # subset selecionado
```

Use quando precisar de publish ad-hoc sem passar por release-plz. Pré-requisito: versões já bumpadas e commit feito.

### Opção C — Manual sequencial (legado v0.3.x)

```bash
git tag -s v<VERSION> -m "Release v<VERSION>"

cargo publish -p serverust-macros
cargo publish -p serverust-core
cargo publish -p serverust-telemetry
cargo publish -p serverust-events
cargo publish -p serverust-lambda
cargo publish -p serverust-cli

git push origin v<VERSION>
```

### Depois do publish

- [ ] CI passou **com a tag** (`git push origin <tag>` dispara workflow no commit do tag)
- [ ] Verificar página do crate em https://crates.io/crates/<name>
- [ ] Documentação publicada em https://docs.rs/<name> (geração automática ~10 min)
- [ ] Anúncio de release em GitHub Releases (opcional, `gh release create`)

---

## Pré-flight obrigatório (Cargo + assinatura)

- Cargo 1.90+ (para `cargo publish --workspace`). Verificar: `cargo --version`.
- SSH signing configurado para tags assinados:
  ```bash
  git config --global gpg.format ssh
  git config --global user.signingkey ~/.ssh/id_<key>.pub
  git config --global tag.gpgsign true
  ```
- Public key adicionada como **Signing key** em https://github.com/settings/ssh/new (diferente de Authentication key).
- `cargo login` configurado (token em `~/.cargo/credentials.toml`).

---

_Referência: CLAUDE.md § Processo de Release_
