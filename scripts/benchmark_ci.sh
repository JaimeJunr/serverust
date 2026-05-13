#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

echo "==> startup smoke (first HTTP response)"
"$BIN_PATH" >/tmp/serverust_bench_stdout.log 2>/tmp/serverust_bench_stderr.log &
APP_PID=$!
cleanup() {
  kill "$APP_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

START_MS="$(date +%s%3N)"
for _ in $(seq 1 100); do
  if curl -sf "http://127.0.0.1:${PORT}/" >/dev/null 2>&1; then
    END_MS="$(date +%s%3N)"
    ELAPSED_MS=$((END_MS - START_MS))
    echo "startup_ms=$ELAPSED_MS (target <= $MAX_STARTUP_MS)"
    if [ "$ELAPSED_MS" -gt "$MAX_STARTUP_MS" ]; then
      echo "ERROR: startup time exceeded target"
      exit 1
    fi
    exit 0
  fi
  sleep 0.05
done

echo "ERROR: server did not become ready in time"
exit 1
