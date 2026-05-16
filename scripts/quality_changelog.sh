#!/usr/bin/env bash
# Falha se a versão no Cargo.toml mudou em relação ao HEAD~1 mas CHANGELOG.md não tem entrada correspondente.
set -euo pipefail

CARGO_TOML="Cargo.toml"
CHANGELOG="CHANGELOG.md"

current_version=$(grep '^version' "$CARGO_TOML" | head -1 | sed 's/.*= *"\(.*\)"/\1/')

if ! grep -qF "## [$current_version]" "$CHANGELOG"; then
    echo "❌ CHANGELOG.md não tem entrada para a versão $current_version." >&2
    echo "   Adicione uma seção '## [$current_version] - YYYY-MM-DD' antes de fazer push." >&2
    exit 1
fi

echo "✅ CHANGELOG.md contém entrada para v$current_version."
