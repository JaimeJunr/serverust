#!/usr/bin/env bash
set -euo pipefail

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "cargo-llvm-cov não encontrado." >&2
  echo "Instale com: cargo install cargo-llvm-cov" >&2
  exit 1
fi

cargo llvm-cov clean --workspace
cargo llvm-cov \
  -p serverust-core \
  -p serverust-cli \
  --fail-under-lines 80 \
  --lcov \
  --output-path coverage.lcov
