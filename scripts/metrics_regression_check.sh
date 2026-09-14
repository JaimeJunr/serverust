#!/usr/bin/env bash
# Compara última entry de history.json com a penúltima e falha se houver regressão.
# Tolerâncias:
#   - stripped_bytes:       regressão > 5% vs versão anterior → falha
#   - startup_local_p50_ms: INFORMATIVO, apenas reportado. A medição local varia
#                           mais com o estado da máquina que com o código (13ms
#                           a 62ms na mesma build) — ver ADR 0008.
# Valores null são ignorados (sem medição = sem regressão detectável).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HISTORY="$ROOT_DIR/docs/product/metrics/history.json"
# shellcheck source=lib/kpi_metrics.sh
source "$ROOT_DIR/scripts/lib/kpi_metrics.sh"

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
  local tolerance_floor="${4:-0}"

  local prev_val curr_val
  prev_val="$(echo "$PREV" | jq "$field")"
  curr_val="$(echo "$CURR" | jq "$field")"

  if [ "$prev_val" = "null" ] || [ "$curr_val" = "null" ]; then
    echo "  $label: sem dados suficientes (null) — ignorado"
    return
  fi

  local exceeded
  exceeded="$(exceeds_tolerance "$prev_val" "$curr_val" "$tolerance_pct" "$tolerance_floor")"

  if [ "$exceeded" = "yes" ]; then
    echo "  FALHOU $label: $prev_val → $curr_val"
    FAILED=1
  else
    echo "  OK    $label: $prev_val → $curr_val"
  fi
}

check_regression ".stripped_bytes" \
  "$BYTES_TOLERANCE_PCT" "stripped_bytes       (tol. ${BYTES_TOLERANCE_PCT}%)" \
  "$BYTES_TOLERANCE_FLOOR"

# Fallback: entradas anteriores ao rename gravavam o startup local como cold_start_p95_ms.
PREV_STARTUP="$(echo "$PREV" | jq '.startup_local_p50_ms // .cold_start_p95_ms')"
CURR_STARTUP="$(echo "$CURR" | jq '.startup_local_p50_ms // .cold_start_p95_ms')"
echo "  INFO  startup_local_p50_ms: $PREV_STARTUP → $CURR_STARTUP (informativo — não reprova)"

if [ "$FAILED" -eq 1 ]; then
  echo ""
  echo "REGRESSÃO DETECTADA. Crie uma ADR em docs/development/decisions/ antes de mergear."
  exit 1
fi

echo "Sem regressões detectadas."
