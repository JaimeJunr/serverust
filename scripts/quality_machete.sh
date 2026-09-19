#!/usr/bin/env bash
# Detecta dependências declaradas e não usadas (cargo-machete).
set -euo pipefail

# shellcheck source=lib/tool_guard.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/tool_guard.sh"

require_tool cargo-machete "dependência não usada" "cargo install cargo-machete" || exit 0

cargo machete
