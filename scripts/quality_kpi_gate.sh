#!/usr/bin/env bash
# Gate de KPI: compara binário stripped e startup local vs baseline em history.json.
#
# Tolerâncias:
#   - stripped_bytes:      falha se tamanho atual for >5% acima da última entry.
#                          Eixo determinístico — mesmo código produz byte-idêntico.
#   - startup_local_p50_ms: INFORMATIVO. Reporta a mediana de N medições e o
#                          delta vs baseline, mas só reprova acima de um teto
#                          absoluto (2000ms). Medições isoladas da mesma build,
#                          sem alteração de código, variaram de 13ms a 62ms na
#                          mesma máquina: comparar contra o baseline nessa
#                          dispersão reprova ruído, não regressão.
#
# ATENÇÃO ao que este eixo NÃO é: `startup_local_p50_ms` mede o tempo até a
# primeira resposta HTTP de um binário local. O invariante público do CLAUDE.md
# — cold start < 50ms no Lambda ARM64 128MB — exige invocação real na AWS e é
# registrado separadamente em `cold_start_p95_ms`. Ver ADR 0008.
#
# Opt-in: controlado por LEFTHOOK_KPI=1
# Override emergência: LEFTHOOK_KPI_SKIP=1 (exige justificativa no commit message — ver CLAUDE.md)
#
# Se a regressão for inevitável, crie uma ADR em docs/development/decisions/ antes de mergear.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HISTORY="$ROOT_DIR/docs/product/metrics/history.json"
# shellcheck source=lib/kpi_metrics.sh
source "$ROOT_DIR/scripts/lib/kpi_metrics.sh"

# Opt-in: só roda se LEFTHOOK_KPI=1
if [ "${LEFTHOOK_KPI:-0}" != "1" ]; then
  echo "KPI gate desativado (LEFTHOOK_KPI != 1). Pulando."
  exit 0
fi

# Override de emergência
if [ "${LEFTHOOK_KPI_SKIP:-0}" = "1" ]; then
  echo "AVISO: KPI gate ignorado via LEFTHOOK_KPI_SKIP=1."
  echo "Certifique-se de que a justificativa está documentada na mensagem do commit."
  exit 0
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "ERRO: jq não encontrado. Instale jq antes de continuar."
  exit 1
fi

COUNT="$(jq 'length' "$HISTORY")"
if [ "$COUNT" -lt 1 ]; then
  echo "INFO: history.json vazio — nada a comparar. Pulando gate."
  exit 0
fi

BASELINE="$(jq '.[-1]' "$HISTORY")"
BASELINE_VERSION="$(echo "$BASELINE" | jq -r '.version')"
echo "Baseline: v$BASELINE_VERSION"

FAILED=0

# --- 1. Build e medir stripped_bytes ---
echo ""
echo "==> Medindo stripped_bytes..."
cargo build --release --manifest-path "$ROOT_DIR/examples/hello-world/Cargo.toml" 2>/dev/null

BIN_PATH="$ROOT_DIR/target/release/hello-world"
STRIPPED_PATH="$BIN_PATH.kpi_stripped"
if command -v strip >/dev/null 2>&1; then
  strip -o "$STRIPPED_PATH" "$BIN_PATH"
else
  cp "$BIN_PATH" "$STRIPPED_PATH"
fi
CURR_BYTES="$(wc -c < "$STRIPPED_PATH" | tr -d ' ')"
rm -f "$STRIPPED_PATH"

BASELINE_BYTES="$(echo "$BASELINE" | jq '.stripped_bytes')"
if [ "$BASELINE_BYTES" != "null" ]; then
  EXCEEDED="$(exceeds_tolerance "$BASELINE_BYTES" "$CURR_BYTES" "$BYTES_TOLERANCE_PCT" "$BYTES_TOLERANCE_FLOOR")"
  if [ "$EXCEEDED" = "yes" ]; then
    echo "  FALHOU stripped_bytes: baseline=$BASELINE_BYTES atual=$CURR_BYTES (tol. ${BYTES_TOLERANCE_PCT}%)"
    echo "  Crie uma ADR em docs/development/decisions/ justificando a regressão."
    FAILED=1
  else
    echo "  OK stripped_bytes: baseline=$BASELINE_BYTES atual=$CURR_BYTES"
  fi
else
  echo "  INFO stripped_bytes: baseline null — ignorando comparação"
fi

# --- 2. Medir startup local (mediana de N amostras) ---
echo ""
echo "==> Medindo startup local (${STARTUP_SAMPLES} amostras)..."
PORT=3000

if ! CURR_STARTUP_MS="$(measure_startup_median_ms "$BIN_PATH" "$PORT")"; then
  echo "  AVISO: servidor não respondeu em tempo — pulando startup check"
else
  echo "  amostras: $(cat "$STARTUP_SAMPLES_FILE") ms -> mediana ${CURR_STARTUP_MS}ms"
  # Entradas anteriores ao rename gravavam este mesmo valor como cold_start_p95_ms.
  BASELINE_STARTUP="$(echo "$BASELINE" | jq '.startup_local_p50_ms // .cold_start_p95_ms')"
  echo "  INFO startup_local: baseline=${BASELINE_STARTUP}ms atual=${CURR_STARTUP_MS}ms (informativo — não reprova)"

  if [ "$(exceeds_absolute "$CURR_STARTUP_MS" "$STARTUP_ABSOLUTE_MAX_MS")" = "yes" ]; then
    echo "  FALHOU startup_local: ${CURR_STARTUP_MS}ms acima do teto absoluto de ${STARTUP_ABSOLUTE_MAX_MS}ms"
    echo "  Teto tão alto só é ultrapassado por regressão grosseira — investigue o caminho de inicialização."
    FAILED=1
  fi
fi

# --- Resultado final ---
echo ""
if [ "$FAILED" -eq 1 ]; then
  echo "KPI GATE FALHOU. Veja as mensagens acima."
  echo "Para justificar uma regressão inevitável, crie uma ADR em:"
  echo "  docs/development/decisions/"
  exit 1
fi

echo "KPI gate passou."
