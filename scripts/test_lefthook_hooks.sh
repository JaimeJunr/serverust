#!/usr/bin/env bash
# Extrai os `run:` reais de lefthook.yml e verifica que ausência da ferramenta
# é skip (exit 0) e reprovação da ferramenta não é mascarada (exit != 0).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LEFTHOOK="$ROOT/lefthook.yml"
BASH_BIN="$(command -v bash)"

TMPDIR_ROOT=""
cleanup() {
  if [[ -n "${TMPDIR_ROOT}" && -d "${TMPDIR_ROOT}" ]]; then
    rm -rf "${TMPDIR_ROOT}"
  fi
}
trap cleanup EXIT

TMPDIR_ROOT="$(mktemp -d)"
EMPTY_BIN="${TMPDIR_ROOT}/empty-bin"
MACHETE_FAIL_BIN="${TMPDIR_ROOT}/machete-fail-bin"
COG_FAIL_BIN="${TMPDIR_ROOT}/cog-fail-bin"
COMMIT_MSG="${TMPDIR_ROOT}/commit-msg.txt"

mkdir -p "$EMPTY_BIN" "$MACHETE_FAIL_BIN" "$COG_FAIL_BIN"
printf '%s\n' "test: mensagem qualquer" > "$COMMIT_MSG"

# Stubs que existem no PATH e saem 1 — ferramenta presente e reprovando.
cat > "${MACHETE_FAIL_BIN}/cargo-machete" <<'EOF'
#!/bin/sh
exit 1
EOF
cat > "${MACHETE_FAIL_BIN}/cargo" <<'EOF'
#!/bin/sh
exit 1
EOF
cat > "${COG_FAIL_BIN}/cog" <<'EOF'
#!/bin/sh
exit 1
EOF
chmod +x "${MACHETE_FAIL_BIN}/cargo-machete" "${MACHETE_FAIL_BIN}/cargo" "${COG_FAIL_BIN}/cog"

extract_run() {
  local section="$1"
  local hook="$2"
  python3 - "$LEFTHOOK" "$section" "$hook" <<'PY'
import sys

try:
    import yaml
except ImportError:
    print(
        "PyYAML ausente: este teste lê os comandos do lefthook.yml real "
        "em vez de uma cópia. Instale com `pip install pyyaml`.",
        file=sys.stderr,
    )
    sys.exit(1)

path, section, hook = sys.argv[1], sys.argv[2], sys.argv[3]
with open(path, encoding="utf-8") as fh:
    data = yaml.safe_load(fh)
try:
    run = data[section]["commands"][hook]["run"]
except (KeyError, TypeError) as exc:
    print(
        f"falha ao extrair {section}.commands.{hook}.run de {path}: {exc}",
        file=sys.stderr,
    )
    sys.exit(1)
if not isinstance(run, str) or not run.strip():
    print(
        f"{section}.commands.{hook}.run vazio ou não-string: {run!r}",
        file=sys.stderr,
    )
    sys.exit(1)
sys.stdout.write(run)
PY
}

run_hook() {
  local cmd="$1"
  local bin_dir="$2"
  PATH="$bin_dir" "$BASH_BIN" -c "$cmd"
}

failures=0

assert_exit() {
  local scenario="$1"
  local got="$2"
  local expect_kind="$3" # zero | nonzero
  local expected_desc
  local ok=0

  if [[ "$expect_kind" == "zero" ]]; then
    expected_desc="0"
    if [[ "$got" -eq 0 ]]; then
      ok=1
    fi
  else
    expected_desc="diferente de 0"
    if [[ "$got" -ne 0 ]]; then
      ok=1
    fi
  fi

  if [[ "$ok" -eq 1 ]]; then
    printf 'OK    %s: exit %s (esperado %s)\n' "$scenario" "$got" "$expected_desc"
  else
    printf 'FALHA %s: exit %s (esperado %s)\n' "$scenario" "$got" "$expected_desc" >&2
    failures=$((failures + 1))
  fi
}

MACHETE_CMD="$(extract_run pre-commit machete)"
COG_CMD="$(extract_run commit-msg cog-verify)"
COG_CMD="${COG_CMD//\{1\}/${COMMIT_MSG}}"

echo "==> comando extraído pre-commit.machete:"
echo "    $MACHETE_CMD"
echo "==> comando extraído commit-msg.cog-verify (após substituir {1}):"
echo "    $COG_CMD"

rc=0
run_hook "$MACHETE_CMD" "$EMPTY_BIN" || rc=$?
assert_exit "machete ausente" "$rc" zero

rc=0
run_hook "$MACHETE_CMD" "$MACHETE_FAIL_BIN" || rc=$?
assert_exit "machete presente e reprovando" "$rc" nonzero

rc=0
run_hook "$COG_CMD" "$EMPTY_BIN" || rc=$?
assert_exit "cog-verify ausente" "$rc" zero

rc=0
run_hook "$COG_CMD" "$COG_FAIL_BIN" || rc=$?
assert_exit "cog-verify presente e reprovando" "$rc" nonzero

if [[ "$failures" -ne 0 ]]; then
  echo "FALHOU: $failures cenário(s) com exit code diferente do esperado." >&2
  exit 1
fi

echo "OK: 4/4 cenários"
exit 0
