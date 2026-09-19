#!/usr/bin/env bash
# Extrai os `run:` reais de lefthook.yml e verifica os TRÊS estados que cada
# gate precisa distinguir (ver scripts/lib/tool_guard.sh):
#   ferramenta ausente, fora de CI  -> exit 0, mas com aviso alto em stderr
#   ferramenta ausente, em CI       -> exit != 0 (CI nunca reporta verde sem verificar)
#   ferramenta presente reprovando  -> exit != 0 (reprovação não é mascarada)
# O aviso em stderr é parte do contrato, não cosmético: sem ele, "não verifiquei"
# volta a ser indistinguível de "verifiquei e passou".
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
FAIL_BIN="${TMPDIR_ROOT}/fail-bin"
COMMIT_MSG="${TMPDIR_ROOT}/commit-msg.txt"

mkdir -p "$EMPTY_BIN" "$FAIL_BIN"
printf '%s\n' "test: mensagem qualquer" > "$COMMIT_MSG"

# Stubs que existem no PATH e saem 1 — ferramenta presente e reprovando.
for stub in cargo cargo-machete cargo-cycles cog; do
  cat > "${FAIL_BIN}/${stub}" <<'EOF'
#!/bin/sh
exit 1
EOF
  chmod +x "${FAIL_BIN}/${stub}"
done

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

# run_hook <comando> <bin_dir> <valor_de_CI> -> ecoa o exit code; stderr vai
# para $STDERR_FILE, para que o teste possa exigir o aviso.
# `/usr/bin:/bin` entra no PATH porque o shebang dos gates precisa de `env` e
# `bash`; nenhuma das ferramentas sob teste (cargo, cargo-machete, cargo-cycles,
# cog) vive lá — elas vêm de ~/.cargo/bin — então a ausência simulada é real.
STDERR_FILE="${TMPDIR_ROOT}/stderr.txt"
run_hook() {
  local cmd="$1"
  local bin_dir="$2"
  local ci_value="$3"
  local rc=0

  # `cd "$ROOT"`: os `run:` do lefthook usam caminho relativo (./scripts/...),
  # exatamente como o lefthook os executa, a partir da raiz do repo.
  if [[ -n "$ci_value" ]]; then
    ( cd "$ROOT" && PATH="${bin_dir}:/usr/bin:/bin" CI="$ci_value" "$BASH_BIN" -c "$cmd" ) \
      >/dev/null 2>"$STDERR_FILE" || rc=$?
  else
    ( cd "$ROOT" && PATH="${bin_dir}:/usr/bin:/bin" env -u CI "$BASH_BIN" -c "$cmd" ) \
      >/dev/null 2>"$STDERR_FILE" || rc=$?
  fi
  echo "$rc"
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

# O que impede o skip silencioso de voltar: exit 0 sozinho não basta, o gate
# precisa ter dito em stderr que não verificou nada.
assert_warns_skip() {
  local scenario="$1"

  if grep -q "PULADO" "$STDERR_FILE" && grep -q "NENHUMA verificação" "$STDERR_FILE"; then
    printf 'OK    %s: aviso de skip presente em stderr\n' "$scenario"
  else
    printf 'FALHA %s: stderr não contém o aviso de skip. stderr foi:\n%s\n' \
      "$scenario" "$(cat "$STDERR_FILE")" >&2
    failures=$((failures + 1))
  fi
}

MACHETE_CMD="$(extract_run pre-commit machete)"
CYCLES_CMD="$(extract_run pre-commit cycles)"
COG_CMD="$(extract_run commit-msg cog-verify)"
COG_CMD="${COG_CMD//\{1\}/${COMMIT_MSG}}"

echo "==> comandos extraídos de lefthook.yml:"
echo "    pre-commit.machete:   $MACHETE_CMD"
echo "    pre-commit.cycles:    $CYCLES_CMD"
echo "    commit-msg.cog-verify: $COG_CMD"

# gate <nome> <comando>: os três estados, para cada gate com dependência externa.
gate() {
  local name="$1"
  local cmd="$2"
  local rc

  rc="$(run_hook "$cmd" "$EMPTY_BIN" "")"
  assert_exit "$name ausente (local)" "$rc" zero
  assert_warns_skip "$name ausente (local)"

  rc="$(run_hook "$cmd" "$EMPTY_BIN" "true")"
  assert_exit "$name ausente (CI=true)" "$rc" nonzero

  rc="$(run_hook "$cmd" "$FAIL_BIN" "")"
  assert_exit "$name presente e reprovando" "$rc" nonzero
}

gate "machete" "$MACHETE_CMD"
gate "cycles" "$CYCLES_CMD"
gate "cog-verify" "$COG_CMD"

if [[ "$failures" -ne 0 ]]; then
  echo "FALHOU: $failures cenário(s) fora do esperado." >&2
  exit 1
fi

echo "OK: todos os cenários"
exit 0
