#!/usr/bin/env bash
set -euo pipefail

if ! cargo mutants --version >/dev/null 2>&1; then
  echo "cargo-mutants não encontrado." >&2
  echo "Instale com: cargo install cargo-mutants" >&2
  exit 1
fi

# Execução enxuta para pre-push: escopo nos crates centrais e modo --check.
cargo mutants \
  --check \
  --in-place \
  --cap-lints=true \
  --package serverust-core \
  --package serverust-cli
