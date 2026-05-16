# Release Checklist — serverust

Checklist obrigatório para toda release. Referência canônica linkada em CLAUDE.md.

---

## Antes do Release

- [ ] `cargo check --workspace` passa sem warnings
- [ ] `cargo test -p serverust-core` passa
- [ ] `cargo test -p serverust-macros` passa
- [ ] `cargo test -p serverust-telemetry` passa
- [ ] `cargo test -p serverust-events` passa
- [ ] `cargo test -p serverust-lambda` passa
- [ ] `scripts/quality_fmt.sh` passa
- [ ] `scripts/quality_lint.sh` passa
- [ ] `scripts/quality_complexity.sh` passa
- [ ] `scripts/quality_cycles.sh` passa
- [ ] `scripts/quality_changelog.sh` passa (versão em Cargo.toml tem entrada em CHANGELOG.md)
- [ ] `scripts/quality_hello_world.sh` passa (hello-world sem deps Kafka/DynamoDB)

## Versão e Changelog

- [ ] Incrementar `version` em `Cargo.toml` workspace
- [ ] Mover itens de `[Unreleased]` para nova versão com data em CHANGELOG.md
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
- [ ] `docs/product/competitors/rocket.md`, `loco.md`, `actix.md` com seção "Kafka & Event Sources" atualizada

## Publicação

- [ ] Tag git criada: `git tag -s v<VERSION> -m "Release v<VERSION>"`
- [ ] Publicar crates na ordem:
  1. `cargo publish -p serverust-macros`
  2. `cargo publish -p serverust-core`
  3. `cargo publish -p serverust-telemetry`
  4. `cargo publish -p serverust-lambda`
  5. `cargo publish -p serverust-events`
  6. `cargo publish -p serverust-cli`
- [ ] CI passou com a tag (verificar GitHub Actions)

---

_Referência: CLAUDE.md § Processo de Release_
