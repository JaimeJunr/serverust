# Release Checklist — serverust

Checklist obrigatório para toda release. Referência canônica linkada em CLAUDE.md.

A partir de v0.4: **per-crate independent versioning** (estilo tokio/axum). Cada crate ganha tag próprio `<crate-name>-vX.Y.Z`. Pré-v0.4 usava workspace-wide unified versioning.

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

- **Per-crate** (default a partir de v0.4): bump apenas o(s) crate(s) que mudaram. Tag `serverust-events-v0.3.1`.
- **Workspace-wide** (legado v0.1.x..v0.3.x): bump `workspace.package.version` em `Cargo.toml`. Tag `v0.3.0`.

### Etapas

- [ ] Incrementar `version` no(s) `Cargo.toml` do(s) crate(s) afetado(s)
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

### Opção A — `cargo-release` (recomendado, atômico)

`cargo-release` é o wrapper que tokio e maintainers Rust top usam. Configurado em `release.toml`.

```bash
# Instalar uma vez:
cargo install cargo-release

# Patch release de um crate específico:
cargo release patch -p serverust-events --execute

# Workspace inteiro (bump major/minor/patch em tudo que mudou):
cargo release patch --workspace --execute

# Dry-run (default, sem --execute):
cargo release patch --workspace
```

`cargo-release` faz tudo em uma corrida: bump → CHANGELOG date → commit → tag SSH-signed (`<crate>-v<X.Y.Z>`) → push → `cargo publish` na ordem certa.

### Opção B — `cargo publish --workspace` (Cargo 1.90+)

A partir de Cargo 1.90 (Nov 2025) o `cargo publish` aceita `--workspace` e resolve ordem de dependência sozinho:

```bash
cargo publish --workspace            # publica TODOS os crates do workspace
cargo publish -p serverust-macros -p serverust-core   # subset selecionado
```

Diferença vs Opção A: não faz bump/tag, só publica. Pré-requisito: versões já bumpadas e commit feito.

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
