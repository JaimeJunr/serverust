# Release Competitive Log

Registro obrigatório por release com dados reais e fontes.

## Template

```markdown
## vX.Y.Z - YYYY-MM-DD

### Referência externa (coletada em YYYY-MM-DD)
- Rocket: <versão> (<fonte>)
- Loco: <versão> (<fonte>)

### Serverust (dados reais da release)
- stripped_size_bytes: <valor>
- lambda_to_axum: <pass/fail>
- quality gates:
  - lint: <pass/fail>
  - complexity: <pass/fail>
  - cycles: <pass/fail>
  - coverage: <pass/fail>
  - mutation: <pass/fail>

### Notas
- <delta principal vs release anterior>
```

---

## v0.1.2 - 2026-05-13

### Referência externa (coletada em 2026-05-13)
- Rocket: `0.5.1` (site oficial indica “Latest Release: 0.5.1 (May 22, 2024)`)
  - Fonte: https://rocket.rs/
- Loco: `0.16.3` (latest em releases do projeto)
  - Fonte: https://github.com/loco-rs/loco/releases

### Serverust (dados reais da release)
- `stripped_size_bytes`: `3545384` (coleta local via `scripts/benchmark_ci.sh`)
- `lambda_to_axum`: `pass (3/3)` cobrindo API Gateway v1, API Gateway v2, Function URL
- quality gates (hooks locais):
  - lint: pass
  - complexity: pass
  - cycles: pass
  - coverage: pass
  - mutation: pass

### Notas
- Compatibilidade IaC formalizada em `docs/guides/iac-compatibility.md`.
- CI de compatibilidade de eventos Lambda adicionado em `.github/workflows/compatibility.yml`.
