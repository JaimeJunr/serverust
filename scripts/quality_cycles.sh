#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=lib/tool_guard.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/tool_guard.sh"

require_tool cargo-cycles "dependência cíclica" "cargo install cargo-cycles" || exit 0

# `cargo-cycles` é binário standalone, NÃO subcomando do cargo: ele rejeita o
# argumento `cycles`. A invocação antiga (`cargo cycles --version`) falhava
# sempre, inclusive com a ferramenta instalada, e caía no fallback silencioso —
# o gate nunca chegou a rodar uma vez.
cargo-cycles
