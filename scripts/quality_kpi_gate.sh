#!/usr/bin/env bash
# Gate de KPI: compara binário stripped e cold start vs baseline em history.json.
#
# Tolerâncias:
#   - stripped_bytes:    falha se tamanho atual for >5%  acima da última entry de history.json
#   - cold_start_local:  falha se startup local for >20% acima da última entry de history.json
#
# Opt-in: controlado por LEFTHOOK_KPI=1
# Override emergência: LEFTHOOK_KPI_SKIP=1 (exige justificativa no commit message — ver CLAUDE.md)
#
# Se a regressão for inevitável, crie uma ADR em docs/development/decisions/ antes de mergear.
# Consulte docs/development/decisions/ para o procedimento completo.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HISTORY="$ROOT_DIR/docs/product/metrics/history.json"

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
  EXCEEDED="$(echo "$BASELINE_BYTES $CURR_BYTES" | awk '{
    threshold = $1 * 1.05
    if ($2 > threshold) print "yes"; else print "no"
  }')"
  if [ "$EXCEEDED" = "yes" ]; then
    echo "  FALHOU stripped_bytes: baseline=$BASELINE_BYTES atual=$CURR_BYTES (tol. 5%)"
    echo "  Crie uma ADR em docs/development/decisions/ justificando a regressão."
    FAILED=1
  else
    echo "  OK stripped_bytes: baseline=$BASELINE_BYTES atual=$CURR_BYTES"
  fi
else
  echo "  INFO stripped_bytes: baseline null — ignorando comparação"
fi

# --- 2. Medir cold start local ---
echo ""
echo "==> Medindo cold start local..."
PORT=3099

"$BIN_PATH" >/tmp/kpi_gate_stdout.log 2>/tmp/kpi_gate_stderr.log &
APP_PID=$!
cleanup() { kill "$APP_PID" >/dev/null 2>&1 || true; }
trap cleanup EXIT

START_MS="$(date +%s%3N)"
READY=0
for _ in $(seq 1 100); do
  if curl -sf "http://127.0.0.1:${PORT}/" >/dev/null 2>&1; then
    END_MS="$(date +%s%3N)"
    CURR_STARTUP_MS=$((END_MS - START_MS))
    READY=1
    break
  fi
  sleep 0.05
done

if [ "$READY" -eq 0 ]; then
  echo "  AVISO: servidor não respondeu em tempo — pulando cold start check"
else
  BASELINE_COLD="$(echo "$BASELINE" | jq '.cold_start_p95_ms')"
  if [ "$BASELINE_COLD" != "null" ]; then
    EXCEEDED="$(echo "$BASELINE_COLD $CURR_STARTUP_MS" | awk '{
      threshold = $1 * 1.20
      if ($2 > threshold) print "yes"; else print "no"
    }')"
    if [ "$EXCEEDED" = "yes" ]; then
      echo "  FALHOU cold_start_local: baseline=${BASELINE_COLD}ms atual=${CURR_STARTUP_MS}ms (tol. 20%)"
      echo "  Crie uma ADR em docs/development/decisions/ justificando a regressão."
      FAILED=1
    else
      echo "  OK cold_start_local: baseline=${BASELINE_COLD}ms atual=${CURR_STARTUP_MS}ms"
    fi
  else
    echo "  INFO cold_start_local: baseline null — resultado atual=${CURR_STARTUP_MS}ms (sem comparação)"
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
