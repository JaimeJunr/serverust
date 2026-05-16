#!/usr/bin/env bash
# Gate obrigatório: a versão corrente do workspace (Cargo.toml) DEVE existir em
# history.json com stripped_bytes e cold_start_p95_ms (ou startup_ms) preenchidos.
#
# Bloqueia release com métricas null. Rode `scripts/metrics_append.sh <version>`
# antes de mergear/taggar.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HISTORY="$ROOT_DIR/docs/product/metrics/history.json"
CARGO_TOML="$ROOT_DIR/Cargo.toml"

if ! command -v jq >/dev/null 2>&1; then
  echo "ERRO: jq não encontrado."
  exit 1
fi

VERSION="$(grep -E '^version' "$CARGO_TOML" | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
if [ -z "$VERSION" ]; then
  echo "ERRO: não consegui ler workspace.version de Cargo.toml"
  exit 1
fi

echo "Workspace version: $VERSION"

ENTRY="$(jq --arg v "$VERSION" '.[] | select(.version == $v)' "$HISTORY")"
if [ -z "$ENTRY" ]; then
  echo "FALHOU: nenhuma entrada para v$VERSION em $HISTORY."
  echo "Rode: scripts/metrics_append.sh $VERSION"
  exit 1
fi

STRIPPED="$(echo "$ENTRY" | jq -r '.stripped_bytes')"
COLD="$(echo "$ENTRY" | jq -r '.cold_start_p95_ms')"

FAILED=0
if [ "$STRIPPED" = "null" ]; then
  echo "FALHOU: stripped_bytes é null para v$VERSION."
  FAILED=1
fi
if [ "$COLD" = "null" ]; then
  echo "FALHOU: cold_start_p95_ms é null para v$VERSION."
  FAILED=1
fi

if [ "$FAILED" -eq 1 ]; then
  echo ""
  echo "Para corrigir:"
  echo "  1. Rode scripts/benchmark_ci.sh para medir hello-world."
  echo "  2. Edite docs/product/metrics/history.json substituindo os null."
  echo "  3. Ou rode scripts/metrics_append.sh <nova-versão> em release nova."
  exit 1
fi

echo "OK: v$VERSION tem stripped_bytes=$STRIPPED e cold_start_p95_ms=$COLD."
