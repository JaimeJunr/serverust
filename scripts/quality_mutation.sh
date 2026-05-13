#!/usr/bin/env bash
set -euo pipefail

if ! cargo mutants --version >/dev/null 2>&1; then
  echo "cargo-mutants não encontrado." >&2
  echo "Instale com: cargo install cargo-mutants" >&2
  exit 1
fi

# Execução padrão enxuta para pre-push; personalize em .cargo/mutants.toml se precisar.
cargo mutants --workspace --in-place
