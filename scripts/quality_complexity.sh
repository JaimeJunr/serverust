#!/usr/bin/env bash
set -euo pipefail

# Heurística prática: usa lint de complexidade cognitiva do clippy como gate.
# Isso mantém o check estável no ecossistema Rust sem parser externo.
cargo clippy --workspace --all-targets -- -W clippy::cognitive_complexity -D warnings
