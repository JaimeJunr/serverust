# Release Checklist (Obrigatório)

Use este checklist em **toda** release (patch/minor/major).

## 1. Preparação

- [ ] Definir versão alvo (`vX.Y.Z`) e branch/tag.
- [ ] Garantir que `README.md` e `docs/INDEX.md` estão atualizados.
- [ ] Rodar quality gates locais:
  - [ ] `./scripts/quality_fmt.sh`
  - [ ] `./scripts/quality_lint.sh`
  - [ ] `./scripts/quality_complexity.sh`
  - [ ] `./scripts/quality_cycles.sh`
  - [ ] `./scripts/quality_coverage.sh`
  - [ ] `./scripts/quality_mutation.sh`

## 2. Compatibilidade de Runtime (IaC-agnóstico)

- [ ] Validar contrato Lambda (eventos reais):
  - [ ] `cargo test -p serverust-lambda --test lambda_to_axum`
- [ ] Confirmar que o guia de compatibilidade continua correto:
  - [ ] `docs/guides/iac-compatibility.md`

## 3. Comparativo Competitivo (Rocket/Loco)

**Obrigatório em toda release.**

- [ ] Atualizar versões e datas mais recentes de Rocket e Loco em:
  - [ ] `docs/product/competitors/release-competitive-log.md`
- [ ] Atualizar fontes (links oficiais) e data da coleta.
- [ ] Atualizar dados reais do serverust para a release:
  - [ ] tamanho do binário stripped (`scripts/benchmark_ci.sh`)
  - [ ] status dos testes de compatibilidade Lambda (v1/v2/Function URL)
  - [ ] status dos quality gates (lint/complexidade/ciclos/cobertura/mutação)

## 4. Publicação

- [ ] Commit final da release (incluindo docs atualizados).
- [ ] Push e CI verde.
- [ ] Criar Git tag/release notes.

## 5. Evidência mínima na release note

Incluir bloco curto com:

- Versão Rocket (fonte + data)
- Versão Loco (fonte + data)
- Métricas serverust coletadas na release
- Data da coleta
