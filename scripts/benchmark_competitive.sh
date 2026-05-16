#!/usr/bin/env bash
# benchmark_competitive.sh — mede LOC, binário stripped e cold start (opt-in)
# para examples/kafka-wallet (serverust) e examples/baselines/axum-raw-kafka (vanilla).
#
# Uso:
#   ./scripts/benchmark_competitive.sh            # LOC + binário local
#   ./scripts/benchmark_competitive.sh --lambda   # inclui cold start via cargo lambda
#
# Saída: JSON em stdout (consumido por docs/product/competitors/release-competitive-log.md)
# Arquivo: docs/product/competitors/competitive-metrics.json (append opcional via --save)

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVERUST_DIR="$ROOT_DIR/examples/kafka-wallet"
BASELINE_DIR="$ROOT_DIR/examples/baselines/axum-raw-kafka"
COMPETITORS_DIR="$ROOT_DIR/docs/product/competitors"
MODE="${1:-}"

# ── helpers ──────────────────────────────────────────────────────────────────

count_loc() {
    local file="$1"
    if [ ! -f "$file" ]; then
        echo "null"
        return
    fi
    # Conta linhas não-vazias e não-comentários (proxy simples para LOC real)
    grep -cE '^\s*[^/[:space:]]' "$file" 2>/dev/null || echo "0"
}

# LOC do handler excluindo bloco #[cfg(test)] em diante
count_handler_loc() {
    local file="$1"
    if [ ! -f "$file" ]; then
        echo "null"
        return
    fi
    # Linhas antes do primeiro #[cfg(test)]
    local test_line
    test_line=$(grep -n '#\[cfg(test)\]' "$file" | head -1 | cut -d: -f1 || echo "")
    if [ -n "$test_line" ]; then
        head -n "$((test_line - 1))" "$file" | grep -cE '^\s*[^/[:space:]]' 2>/dev/null || echo "0"
    else
        grep -cE '^\s*[^/[:space:]]' "$file" 2>/dev/null || echo "0"
    fi
}

build_and_strip() {
    local manifest="$1"
    local bin_name="$2"
    local bin_path="$ROOT_DIR/target/release/$bin_name"
    local stripped="$bin_path.stripped"

    if ! cargo build --release --manifest-path "$manifest" 2>/dev/null; then
        echo "null"
        return
    fi

    if command -v strip >/dev/null 2>&1; then
        strip -o "$stripped" "$bin_path" 2>/dev/null || cp "$bin_path" "$stripped"
    else
        cp "$bin_path" "$stripped"
    fi

    wc -c < "$stripped" | tr -d ' '
}

cold_start_local() {
    local bin="$1"
    local port="${2:-19876}"
    if [ ! -f "$bin" ]; then
        echo "null"
        return
    fi
    "$bin" >/dev/null 2>&1 &
    local pid=$!
    local start_ms; start_ms="$(date +%s%3N)"
    local elapsed="null"
    for _ in $(seq 1 60); do
        if curl -sf "http://127.0.0.1:$port/" >/dev/null 2>&1; then
            elapsed=$(($(date +%s%3N) - start_ms))
            break
        fi
        sleep 0.05
    done
    kill "$pid" 2>/dev/null || true
    echo "$elapsed"
}

# ── coleta serverust kafka-wallet ─────────────────────────────────────────────

echo "==> Coletando métricas: serverust kafka-wallet" >&2

SERVERUST_HANDLER_FILE="$SERVERUST_DIR/src/main.rs"
# kafka-wallet é uma Lambda direta — o handler está em src/main.rs ou src/handler.rs
if [ -f "$SERVERUST_DIR/src/handler.rs" ]; then
    SERVERUST_HANDLER_FILE="$SERVERUST_DIR/src/handler.rs"
fi

SERVERUST_LOC=$(count_handler_loc "$SERVERUST_HANDLER_FILE")

SERVERUST_STRIPPED="null"
if [ -f "$SERVERUST_DIR/Cargo.toml" ]; then
    echo "  building kafka-wallet..." >&2
    SERVERUST_STRIPPED=$(build_and_strip "$SERVERUST_DIR/Cargo.toml" "kafka-wallet" 2>/dev/null || echo "null")
fi

# ── coleta baseline axum-raw-kafka ────────────────────────────────────────────

echo "==> Coletando métricas: baseline axum-raw-kafka" >&2

BASELINE_HANDLER_FILE="$BASELINE_DIR/src/handler.rs"
BASELINE_LOC=$(count_handler_loc "$BASELINE_HANDLER_FILE")

BASELINE_STRIPPED="null"
if [ -f "$BASELINE_DIR/Cargo.toml" ]; then
    echo "  building axum-raw-kafka..." >&2
    BASELINE_STRIPPED=$(build_and_strip "$BASELINE_DIR/Cargo.toml" "axum-raw-kafka" 2>/dev/null || echo "null")
fi

# ── cold start (opt-in --lambda) ─────────────────────────────────────────────

SERVERUST_COLD="null"
BASELINE_COLD="null"

if [ "$MODE" = "--lambda" ]; then
    echo "==> Medindo cold start local (HTTP mode)..." >&2
    if [ -f "$ROOT_DIR/target/release/kafka-wallet" ]; then
        SERVERUST_COLD=$(cold_start_local "$ROOT_DIR/target/release/kafka-wallet" 19876)
    fi
    if [ -f "$ROOT_DIR/target/release/axum-raw-kafka" ]; then
        BASELINE_COLD=$(cold_start_local "$ROOT_DIR/target/release/axum-raw-kafka" 19877)
    fi
fi

# ── calcula ratio LOC ─────────────────────────────────────────────────────────

LOC_RATIO="null"
if [ "$SERVERUST_LOC" != "null" ] && [ "$BASELINE_LOC" != "null" ] \
   && [ "$SERVERUST_LOC" -gt 0 ] && [ "$BASELINE_LOC" -gt 0 ]; then
    LOC_RATIO=$(echo "scale=2; $BASELINE_LOC / $SERVERUST_LOC" | bc)
fi

# ── output JSON ───────────────────────────────────────────────────────────────

DATE=$(date +"%Y-%m-%d")
OUTPUT=$(cat <<EOF
{
  "date": "$DATE",
  "serverust_kafka_wallet": {
    "loc_handler": $SERVERUST_LOC,
    "stripped_bytes": $SERVERUST_STRIPPED,
    "cold_start_local_ms": $SERVERUST_COLD,
    "notes": "examples/kafka-wallet — handler com #[kafka_consumer], KafkaProducer, DynamoRepo<T>"
  },
  "baseline_axum_raw_kafka": {
    "loc_handler": $BASELINE_LOC,
    "stripped_bytes": $BASELINE_STRIPPED,
    "cold_start_local_ms": $BASELINE_COLD,
    "notes": "examples/baselines/axum-raw-kafka — lambda_runtime + rdkafka + aws-sdk-dynamodb vanilla"
  },
  "loc_ratio_baseline_over_serverust": $LOC_RATIO,
  "legend": {
    "loc_handler": "linhas não-vazias/comentário no arquivo handler, excluindo #[cfg(test)]",
    "stripped_bytes": "tamanho do binário release após strip",
    "cold_start_local_ms": "tempo até primeira resposta HTTP local (null = não medido)"
  }
}
EOF
)

echo "$OUTPUT"

# ── save opcional ─────────────────────────────────────────────────────────────

if [ "$MODE" = "--save" ] || [ "${2:-}" = "--save" ]; then
    OUTFILE="$COMPETITORS_DIR/competitive-metrics.json"
    echo "$OUTPUT" > "$OUTFILE"
    echo "==> Salvo em $OUTFILE" >&2
fi
