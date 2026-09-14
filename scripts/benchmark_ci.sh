#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=lib/kpi_metrics.sh
source "$ROOT_DIR/scripts/lib/kpi_metrics.sh"

BIN_PATH="$ROOT_DIR/target/release/hello-world"
STRIPPED_PATH="$BIN_PATH.stripped"
MAX_BIN_BYTES=$((10 * 1024 * 1024))
MAX_STARTUP_MS=2000
PORT=3000

echo "==> build hello-world release"
cargo build --release --manifest-path "$ROOT_DIR/examples/hello-world/Cargo.toml"

if command -v strip >/dev/null 2>&1; then
  strip -o "$STRIPPED_PATH" "$BIN_PATH" || cp "$BIN_PATH" "$STRIPPED_PATH"
else
  cp "$BIN_PATH" "$STRIPPED_PATH"
fi

BIN_BYTES="$(wc -c < "$STRIPPED_PATH" | tr -d ' ')"
echo "stripped_size_bytes=$BIN_BYTES (target <= $MAX_BIN_BYTES)"
if [ "$BIN_BYTES" -gt "$MAX_BIN_BYTES" ]; then
  echo "ERROR: stripped binary exceeded 10MB target"
  exit 1
fi

# Mediana de N amostras: uma medição única cai em qualquer ponto do ruído de
# agendamento do SO e vira baseline enviesado para todas as execuções seguintes.
echo "==> startup smoke (first HTTP response, ${STARTUP_SAMPLES} samples)"
if ! ELAPSED_MS="$(measure_startup_median_ms "$BIN_PATH" "$PORT")"; then
  echo "ERROR: server did not become ready in time"
  exit 1
fi

SAMPLES_RAW="$(cat "$STARTUP_SAMPLES_FILE")"
echo "startup_local_samples_ms=${SAMPLES_RAW// /,}"
echo "startup_local_samples=${STARTUP_SAMPLES}"
echo "startup_local_p50_ms=$ELAPSED_MS (target <= $MAX_STARTUP_MS)"
if [ "$ELAPSED_MS" -gt "$MAX_STARTUP_MS" ]; then
  echo "ERROR: startup time exceeded target"
  exit 1
fi
exit 0
