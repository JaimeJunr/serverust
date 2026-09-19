#!/usr/bin/env bash
# Valida a mensagem de commit contra Conventional Commits via cocogitto (cog.toml).
# Uso: quality_commit_msg.sh <arquivo-da-mensagem>
set -euo pipefail

# shellcheck source=lib/tool_guard.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/tool_guard.sh"

MSG_FILE="${1:?uso: quality_commit_msg.sh <arquivo-da-mensagem>}"

INSTALL="curl https://raw.githubusercontent.com/cocogitto/cocogitto/main/install.sh | bash"
require_tool cog "Conventional Commits na mensagem" "$INSTALL" || exit 0

cog verify --file "$MSG_FILE"
