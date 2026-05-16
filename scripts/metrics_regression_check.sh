#!/usr/bin/env bash
# Compara última entry de history.json com a penúltima e falha se houver regressão.
# Tolerâncias:
#   - cold_start_p95_ms: regressão > 10% em relação à versão anterior → falha
#   - stripped_bytes:    regressão > 5%  em relação à versão anterior → falha
# Valores null são ignorados (sem medição = sem regressão detectável).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HISTORY="$ROOT_DIR/docs/product/metrics/history.json"

if ! command -v jq >/dev/null 2>&1; then
  echo "ERRO: jq não encontrado."
  exit 1
fi

COUNT="$(jq 'length' "$HISTORY")"
if [ "$COUNT" -lt 2 ]; then
  echo "INFO: menos de 2 entradas em history.json — nada a comparar."
  exit 0
fi

PREV="$(jq '.[-2]' "$HISTORY")"
CURR="$(jq '.[-1]' "$HISTORY")"

PREV_VERSION="$(echo "$PREV" | jq -r '.version')"
CURR_VERSION="$(echo "$CURR" | jq -r '.version')"
echo "Comparando v$CURR_VERSION (atual) vs v$PREV_VERSION (anterior)"

FAILED=0

check_regression() {
  local field="$1"
  local tolerance_pct="$2"
  local label="$3"

  local prev_val curr_val
  prev_val="$(echo "$PREV" | jq ".$field")"
  curr_val="$(echo "$CURR" | jq ".$field")"

  if [ "$prev_val" = "null" ] || [ "$curr_val" = "null" ]; then
    echo "  $label: sem dados suficientes (null) — ignorado"
    return
  fi

  # Regressão = curr > prev * (1 + tolerance/100)
  local exceeded
  exceeded="$(echo "$prev_val $curr_val $tolerance_pct" | awk '{
    threshold = $1 * (1 + $3/100)
    if ($2 > threshold) print "yes"; else print "no"
  }')"

  if [ "$exceeded" = "yes" ]; then
    echo "  FALHOU $label: $prev_val → $curr_val (tolerância $tolerance_pct%)"
    FAILED=1
  else
    echo "  OK    $label: $prev_val → $curr_val"
  fi
}

check_regression "cold_start_p95_ms" 10 "cold_start_p95_ms (tol. 10%)"
check_regression "stripped_bytes"     5  "stripped_bytes     (tol. 5%)"

if [ "$FAILED" -eq 1 ]; then
  echo ""
  echo "REGRESSÃO DETECTADA. Crie uma ADR em docs/development/decisions/ antes de mergear."
  exit 1
fi

echo "Sem regressões detectadas."
