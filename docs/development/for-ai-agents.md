# for-ai-agents.md — Guia Rápido para AI Agents

> Máquina-legível. Máximo 200 linhas. Leia antes de qualquer mudança.

---

## Antes de qualquer mudança

Execute nesta ordem antes de modificar qualquer arquivo:

```bash
# 1. Verificar invariantes do projeto
cat CLAUDE.md

# 2. Ler ADRs relevantes ao domínio que será alterado
ls docs/development/decisions/

# 3. Verificar typecheck do crate afetado ANTES de alterar
cargo check -p <crate-afetado>

# 4. Para mudanças em docs/product/metrics/: ler histórico de KPIs
cat docs/product/metrics/history.json
```

---

## Comandos seguros

Estes comandos são somente-leitura ou não destrutivos — podem ser executados livremente:

```bash
cargo check -p <crate>          # typecheck isolado, não faz build completo
cargo test -p <crate>           # testes de um único crate
cargo tree -p <crate>           # visualiza árvore de dependências
scripts/quality_changelog.sh    # valida CHANGELOG.md
scripts/quality_kpi_gate.sh     # valida KPIs contra baseline histórico
scripts/benchmark_ci.sh         # mede binário e cold start (leitura)
```

**Nunca** rode `cargo build --workspace` ou `cargo test --workspace` — consome disco e RAM em excesso.

---

## Arquivos que requerem aprovação humana

Estes arquivos **não devem ser modificados por AI agents sem revisão explícita do mantenedor**:

| Arquivo | Motivo |
|---|---|
| `CHANGELOG.md` — entrada de versão final (ex: `## [0.2.0]`) | Versão oficial; só o mantenedor decide quando e o que entra |
| `docs/development/decisions/*.md` — ADRs com `Status: accepted` | Decisões arquiteturais ratificadas; alterar requer nova ADR |
| `docs/product/competitors/release-competitive-log.md` | Log de release público; impacto externo |
| `Cargo.toml` raiz — campo `version` | Bump de versão é ato intencional do mantenedor |

---

## Como medir antes/depois

Toda mudança que pode afetar performance ou tamanho de binário deve ser medida:

```bash
# 1. Medir estado atual (antes)
scripts/benchmark_ci.sh > /tmp/before.txt

# 2. Aplicar a mudança

# 3. Medir estado após
scripts/benchmark_ci.sh > /tmp/after.txt

# 4. Comparar com baseline histórico
cat docs/product/metrics/history.json
diff /tmp/before.txt /tmp/after.txt
```

**Regressão = qualquer métrica piorando além dos limites em `CLAUDE.md > Invariantes`.**
Se uma regressão for inevitável, crie uma ADR em `docs/development/decisions/` antes de mergear.

---

## Exemplo de fluxo: adicionar nova feature em serverust-events

```bash
# 1. Ler o estado atual do crate
cargo check -p serverust-events
cat crates/serverust-events/src/lib.rs

# 2. Verificar ADRs relacionadas a eventos
cat docs/development/decisions/0003-event-driven-separate-crate.md

# 3. Escrever o teste ANTES do código (TDD obrigatório)
# Arquivo: crates/serverust-events/tests/minha_feature.rs

# 4. Confirmar que o teste FALHA (RED)
cargo test -p serverust-events -- minha_feature

# 5. Implementar o mínimo para passar (GREEN)
# Editar: crates/serverust-events/src/lib.rs (ou novo módulo)

# 6. Confirmar que o teste PASSA
cargo test -p serverust-events

# 7. Verificar que serverust-core não ganhou dependências indesejadas
cargo tree -p serverust-core | grep -E "kafka|rdkafka|event"

# 8. Typecheck do workspace (leve — só resolve tipos, não compila)
cargo check --workspace

# 9. Rodar quality gates
scripts/quality_changelog.sh
scripts/quality_kpi_gate.sh   # se existir

# 10. Commitar com mensagem convencional
git commit -m "feat(serverust-events): <descrição curta>"
```

---

## Invariantes que nunca podem regredir

| Invariante | Limite |
|---|---|
| Cold start ARM64 128 MB (`hello-world`) | < 50 ms p95 |
| Binário stripped (`hello-world`) | < 10 MB |
| `serverust-core` sem deps Kafka/eventos | zero |
| `hello-world` sem deps Kafka/DynamoDB | zero |
| Typecheck do workspace | verde |

Fonte autoritativa: `CLAUDE.md > Invariantes`.
