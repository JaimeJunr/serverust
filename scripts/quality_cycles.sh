#!/usr/bin/env bash
set -euo pipefail

if cargo cycles --version >/dev/null 2>&1; then
  cargo cycles
  exit 0
fi

echo cargo-cycles