#!/usr/bin/env bash
# Guarda o gerador da matriz de testes do CI.
#
# O gerador existe para que cobertura de CI seja consequência de existir no
# workspace. Estes testes guardam as duas formas de essa promessa se perder
# em silêncio — que é o modo de falha que ela veio corrigir.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/.."

falhas=0

reprova() {
  echo "FALHOU: $1" >&2
  falhas=$((falhas + 1))
}

matriz="$(./scripts/ci_test_matrix.sh)"
metadata="$(cargo metadata --no-deps --format-version 1)"

# ---------------------------------------------------------------------------
# 1. Todo membro do workspace aparece na matriz
# ---------------------------------------------------------------------------
# Se isto cair, algum crate voltou a ficar sem CI — o buraco original.

membros="$(jq -r '.packages[].name' <<<"$metadata" | sort -u)"
na_matriz="$(jq -r '.include[].crate' <<<"$matriz" | sort -u)"

ausentes="$(comm -23 <(echo "$membros") <(echo "$na_matriz"))"
if [[ -n "$ausentes" ]]; then
  reprova "membros do workspace fora da matriz de testes: $(echo "$ausentes" | tr '\n' ' ')"
fi

# ---------------------------------------------------------------------------
# 2. A lista de exceções não cobre nada além do necessário
# ---------------------------------------------------------------------------
# `no_tests=pass` faz o nextest sair 0 sem rodar nada. Numa lista escrita à
# mão, uma entrada que envelhece vira exatamente o verde vazio que o resto
# desta pipeline recusa: o crate ganha testes, a exceção continua lá, e as
# falhas dele param de ser vistas.

dispensados="$(jq -r '.include[] | select(.no_tests == "pass") | .crate' <<<"$matriz" | sort -u)"

for crate in $dispensados; do
  alvos="$(jq -r --arg c "$crate" '
    .packages[] | select(.name == $c) | [.targets[] | select(.kind | index("test"))] | length
  ' <<<"$metadata")"

  if [[ "$alvos" != "0" ]]; then
    reprova "'$crate' está dispensado de ter testes, mas tem ${alvos} alvo(s) de teste — \
a dispensa esconderia as falhas deles. Remova-o de SEM_TESTES em scripts/ci_test_matrix.sh."
  fi
done

# ---------------------------------------------------------------------------
# 3. Crate com zero testes precisa estar dispensado explicitamente
# ---------------------------------------------------------------------------
# O contrário do anterior: sem isto, o CI reprova o crate com uma mensagem de
# nextest sobre "no tests to run", que não diz onde declarar a exceção.

for crate in $membros; do
  alvos="$(jq -r --arg c "$crate" '
    .packages[] | select(.name == $c) | [.targets[] | select(.kind | index("test"))] | length
  ' <<<"$metadata")"

  if [[ "$alvos" == "0" ]] && ! grep -qx "$crate" <<<"$dispensados"; then
    reprova "'$crate' não tem nenhum alvo de teste e não está em SEM_TESTES. \
Escreva um teste, ou declare a exceção com o motivo em scripts/ci_test_matrix.sh."
  fi
done

if [[ "$falhas" -gt 0 ]]; then
  echo "${falhas} verificação(ões) da matriz de CI falharam." >&2
  exit 1
fi

echo "ok: matriz de CI cobre os $(wc -l <<<"$membros") membros do workspace"
