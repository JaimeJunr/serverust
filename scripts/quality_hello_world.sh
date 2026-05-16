#!/usr/bin/env bash
# Garante que examples/hello-world continua sem deps de Kafka/DynamoDB e compila corretamente.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "==> cargo check hello-world"
cargo check --manifest-path "$ROOT_DIR/examples/hello-world/Cargo.toml" -q

echo "==> verificando ausência de deps Kafka/DynamoDB em hello-world"
FORBIDDEN="$(cargo tree -p hello-world --manifest-path "$ROOT_DIR/examples/hello-world/Cargo.toml" \
  | grep -iE 'kafka|rdkafka|aws-sdk-dynamodb' || true)"

if [ -n "$FORBIDDEN" ]; then
  echo "ERRO: deps proibidas encontradas em hello-world:"
  echo "$FORBIDDEN"
  echo "hello-world deve permanecer HTTP-only sem deps de Kafka/DynamoDB."
  exit 1
fi

echo "OK: hello-world limpo (sem kafka, sem dynamodb)"
