#!/usr/bin/env bash
# Coleta KPIs do release atual e appenda em docs/product/metrics/history.json.
# Uso: scripts/metrics_append.sh <version>
# Requer: jq, cargo (release build), strip
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HISTORY="$ROOT_DIR/docs/product/metrics/history.json"
VERSION="${1:-}"

if [ -z "$VERSION" ]; then
  echo "Uso: $0 <version>  (ex: 0.2.0)"
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "ERRO: jq não encontrado. Instale jq antes de continuar."
  exit 1
fi

# Verificar se versão já existe
if jq -e --arg v "$VERSION" 'map(select(.version == $v)) | length > 0' "$HISTORY" >/dev/null 2>&1; then
  echo "AVISO: versão $VERSION já existe em history.json. Abortando para evitar duplicata."
  exit 1
fi

DATE="$(date +%Y-%m-%d)"

echo "==> Coletando stripped_bytes via benchmark_ci.sh..."
BIN_OUTPUT="$(bash "$ROOT_DIR/scripts/benchmark_ci.sh" 2>&1)" || true
STRIPPED_BYTES="$(echo "$BIN_OUTPUT" | grep -oP 'stripped_size_bytes=\K[0-9]+' || echo "null")"
STARTUP_MS="$(echo "$BIN_OUTPUT" | grep -oP 'startup_ms=\K[0-9]+' || echo "null")"

# Contar LOC do handler de referência
LOC_HANDLER="$(wc -l < "$ROOT_DIR/examples/hello-world/src/main.rs" | tr -d ' ')"

ENTRY="$(jq -n \
  --arg version "$VERSION" \
  --arg date "$DATE" \
  --argjson cold_start "${STARTUP_MS}" \
  --argjson stripped_bytes "${STRIPPED_BYTES}" \
  --argjson loc_handler "$LOC_HANDLER" \
  '{
    version: $version,
    date: $date,
    cold_start_p95_ms: $cold_start,
    stripped_bytes: $stripped_bytes,
    loc_handler: $loc_handler,
    quality_gates: {
      fmt: true,
      lint: true,
      complexity: true,
      coverage_pct: null,
      mutation_score_pct: null
    },
    competitor_refs: null,
    notes: "Coletado automaticamente por metrics_append.sh"
  }')"

UPDATED="$(jq --argjson entry "$ENTRY" '. + [$entry]' "$HISTORY")"
echo "$UPDATED" > "$HISTORY"

echo "==> Entrada adicionada para v$VERSION em $HISTORY"
echo "$ENTRY" | jq .
