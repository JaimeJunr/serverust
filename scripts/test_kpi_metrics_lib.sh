#!/usr/bin/env bash
# Testa a lógica compartilhada dos gates de KPI: tolerância com piso absoluto
# e agregação de amostras de startup.
#
# Motivação: o eixo de tempo reprovava aleatoriamente. Medições isoladas da
# mesma build, sem alteração de código, variaram de 13ms a 62ms na mesma
# máquina — dispersão que nenhuma tolerância relativa ao baseline sobrevive.
# O tempo passou a ser guardado por teto absoluto; ver ADR 0008.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=lib/kpi_metrics.sh
source "$ROOT/scripts/lib/kpi_metrics.sh"

failures=0

assert_eq() {
  local scenario="$1" got="$2" want="$3"
  if [[ "$got" == "$want" ]]; then
    printf 'OK    %s: %s\n' "$scenario" "$got"
  else
    printf 'FALHA %s: obtido %s, esperado %s\n' "$scenario" "$got" "$want" >&2
    failures=$((failures + 1))
  fi
}

# --- exceeds_absolute: guarda do eixo de tempo ---
# Toda a faixa de ruído observada (13ms a 62ms, mesma build) passa.
for observed in 13 14 16 21 24 35 62; do
  assert_eq "startup ${observed}ms dentro do teto" \
    "$(exceeds_absolute "$observed" "$STARTUP_ABSOLUTE_MAX_MS")" "no"
done
# Regressão grosseira — dep pesada no caminho de inicialização — é pega.
assert_eq "startup 2500ms acima do teto" \
  "$(exceeds_absolute 2500 "$STARTUP_ABSOLUTE_MAX_MS")" "yes"
assert_eq "startup no teto exato" \
  "$(exceeds_absolute 2000 "$STARTUP_ABSOLUTE_MAX_MS")" "no"

# --- exceeds_tolerance: em valores grandes o percentual domina o piso ---
# stripped_bytes usa piso 0: o eixo é determinístico e não precisa de folga.
assert_eq "3550112 bytes idêntico" "$(exceeds_tolerance 3550112 3550112 5 0)" "no"
assert_eq "3550112 -> +4% bytes" "$(exceeds_tolerance 3550112 3692116 5 0)" "no"
assert_eq "3550112 -> +6% bytes" "$(exceeds_tolerance 3550112 3763118 5 0)" "yes"

# --- median_of: agregação de N amostras ---
assert_eq "mediana ímpar" "$(median_of 16 11 13)" "13"
assert_eq "mediana par (elemento inferior)" "$(median_of 20 10 12 14)" "12"
assert_eq "mediana amostra única" "$(median_of 11)" "11"
assert_eq "mediana ignora ordem de entrada" "$(median_of 13 11 16 14 12)" "13"

if [[ "$failures" -ne 0 ]]; then
  echo "FALHOU: $failures asserção(ões)." >&2
  exit 1
fi

echo "OK: todas as asserções passaram"
exit 0
