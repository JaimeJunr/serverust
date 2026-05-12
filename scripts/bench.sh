#!/usr/bin/env bash
# bench.sh — benchmark de cold start e tamanho de binário do hello-world
#
# Requisitos locais:
#   - rustup com target aarch64-unknown-linux-musl instalado
#   - cargo-lambda (para deploy Lambda): cargo install cargo-lambda
#
# Requisitos Lambda (benchmark real de cold start):
#   - AWS CLI configurado com credenciais
#   - Função Lambda criada previamente (veja README.md#deploy-lambda)
#
# Uso:
#   ./scripts/bench.sh [--lambda]   # --lambda inclui benchmark de cold start

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
HELLO_WORLD_DIR="$WORKSPACE_DIR/examples/hello-world"
TARGET_DIR="$WORKSPACE_DIR/target"
LAMBDA_FUNCTION="${LAMBDA_FUNCTION_NAME:-serverust-hello-world-bench}"
BENCH_LAMBDA="${1:-}"

echo "============================================================"
echo "  serverust Framework — Benchmark"
echo "============================================================"

# ── 1. Build release (ARM64 para Lambda) ────────────────────────
echo ""
echo "▶ Build release (aarch64-unknown-linux-musl)..."

cd "$WORKSPACE_DIR"
cargo build --release \
    --manifest-path "$HELLO_WORLD_DIR/Cargo.toml" \
    --target aarch64-unknown-linux-musl \
    2>&1 | tail -5 || {
    echo "⚠  Build ARM64 falhou — tentando x86_64..."
    cargo build --release \
        --manifest-path "$HELLO_WORLD_DIR/Cargo.toml" 2>&1 | tail -5
    BINARY="$TARGET_DIR/release/hello-world"
}

BINARY_ARM="${TARGET_DIR}/aarch64-unknown-linux-musl/release/hello-world"
BINARY_X86="${TARGET_DIR}/release/hello-world"
BINARY="${BINARY_ARM:-$BINARY_X86}"
[ -f "$BINARY_ARM" ] && BINARY="$BINARY_ARM" || BINARY="$BINARY_X86"

# ── 2. Tamanho do binário (stripped) ─────────────────────────────
echo ""
echo "▶ Tamanho do binário..."

BINARY_STRIPPED="$BINARY.stripped"
strip -o "$BINARY_STRIPPED" "$BINARY" 2>/dev/null || cp "$BINARY" "$BINARY_STRIPPED"

SIZE_UNSTRIPPED=$(du -sh "$BINARY" 2>/dev/null | cut -f1)
SIZE_STRIPPED=$(du -sh "$BINARY_STRIPPED" 2>/dev/null | cut -f1)
SIZE_BYTES=$(stat -c%s "$BINARY_STRIPPED" 2>/dev/null || stat -f%z "$BINARY_STRIPPED" 2>/dev/null || echo 0)
SIZE_MB=$(echo "scale=2; $SIZE_BYTES / 1048576" | bc 2>/dev/null || echo "N/A")

echo "  Binário (não stripped): $SIZE_UNSTRIPPED"
echo "  Binário (stripped):     $SIZE_STRIPPED  (${SIZE_MB} MB)"

if [ "$SIZE_BYTES" -gt 0 ] && [ "$SIZE_BYTES" -lt 10485760 ]; then
    echo "  ✅ Tamanho dentro do alvo: < 10 MB"
else
    echo "  ⚠  Tamanho acima do alvo de 10 MB"
fi

# ── 3. Startup local (estimativa) ────────────────────────────────
echo ""
echo "▶ Startup local (HTTP mode)..."

# Sobe o servidor em background e mede tempo até primeira resposta
STARTUP_START=$(date +%s%N)
PORT=18765
BINARY_LOCAL="$TARGET_DIR/release/hello-world"
if [ -f "$BINARY_LOCAL" ]; then
    "$BINARY_LOCAL" &
    SERVER_PID=$!
    for i in $(seq 1 50); do
        if curl -sf "http://127.0.0.1:$PORT/" >/dev/null 2>&1; then
            STARTUP_END=$(date +%s%N)
            STARTUP_MS=$(( (STARTUP_END - STARTUP_START) / 1000000 ))
            echo "  Startup local: ${STARTUP_MS} ms (até primeira resposta)"
            break
        fi
        sleep 0.05
    done
    kill $SERVER_PID 2>/dev/null || true
else
    echo "  (binário x86_64 não encontrado — execute primeiro: cargo build --release -p hello-world)"
fi

# ── 4. Cold start Lambda (opcional, requer --lambda) ─────────────
if [ "$BENCH_LAMBDA" = "--lambda" ]; then
    echo ""
    echo "▶ Cold start Lambda ARM64 128MB..."

    if ! command -v aws >/dev/null 2>&1; then
        echo "  ⚠  AWS CLI não encontrado — skipping"
    else
        # Força cold start via atualização de environment variable
        echo "  Forçando cold start via update de env var..."
        aws lambda update-function-configuration \
            --function-name "$LAMBDA_FUNCTION" \
            --environment "Variables={BENCH_RUN=$(date +%s)}" \
            --output text --query 'LastUpdateStatus' 2>&1 || echo "  ⚠  Falha ao atualizar função"

        sleep 2  # aguardar propagação

        echo "  Invocando função..."
        INVOKE_RESULT=$(aws lambda invoke \
            --function-name "$LAMBDA_FUNCTION" \
            --payload '{"httpMethod":"GET","path":"/","headers":{}}' \
            --log-type Tail \
            --output json \
            /tmp/lambda-response.json 2>&1)

        INIT_DURATION=$(echo "$INVOKE_RESULT" | grep -o 'Init Duration: [0-9.]*' | awk '{print $3}' || echo "N/A")
        BILLED_DURATION=$(echo "$INVOKE_RESULT" | grep -o 'Billed Duration: [0-9]*' | awk '{print $3}' || echo "N/A")

        echo "  Init Duration (cold start): ${INIT_DURATION} ms"
        echo "  Billed Duration:            ${BILLED_DURATION} ms"

        if [ "$INIT_DURATION" != "N/A" ]; then
            TARGET=50
            if (( $(echo "$INIT_DURATION < $TARGET" | bc -l 2>/dev/null || echo 0) )); then
                echo "  ✅ Cold start dentro do alvo: < ${TARGET}ms"
            else
                echo "  ⚠  Cold start acima do alvo de ${TARGET}ms"
            fi
        fi
    fi
else
    echo ""
    echo "  ℹ  Para benchmark de cold start em Lambda:"
    echo "     1. Deploy: cargo lambda deploy -p hello-world --arm64"
    echo "     2. Execute: ./scripts/bench.sh --lambda"
fi

echo ""
echo "============================================================"
echo "  Benchmark concluído"
echo "============================================================"
