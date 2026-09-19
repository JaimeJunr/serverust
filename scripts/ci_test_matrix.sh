#!/usr/bin/env bash
# Deriva a matriz de testes do CI a partir do workspace, em vez de mantê-la à
# mão em `.github/workflows/tests.yml`.
#
# # Por que isto existe
#
# A matriz escrita à mão é uma allowlist por presença: um crate novo que
# ninguém lembra de adicionar não reprova — ele some. Foi o que aconteceu com
# o `serverust-auth`, criado com 65 testes (todos os de default deny entre
# eles) que nunca rodaram no CI, porque faltava uma linha de YAML que nenhum
# `grep` procura.
#
# É a regra 3 do corolário de docs/product/philosophy.md aplicada à própria
# pipeline: garantia que depende de alguém lembrar custa caro no milésimo
# crate e falha em silêncio antes disso. Aqui a cobertura passa a ser
# consequência de existir no workspace.
#
# # As duas listas que sobraram
#
# Nenhuma das duas pode esconder um crate:
#
# - `SEM_TESTES` — crates autorizados a não ter nenhum teste. Esquecer de
#   incluir alguém aqui faz o `nextest` reprovar por 0 testes, ou seja, falha
#   alto. É o mesmo motivo de `--no-tests=pass` nunca ser aplicado à matriz
#   inteira.
# - `COMBINACOES_EXTRA` — combinações de features além do default. Esquecer uma
#   reduz cobertura, mas o crate continua sendo testado com as features
#   default: degrada, não desaparece.
#
# A necessidade de toolchain de C (librdkafka) é derivada das dependências
# reais do crate, não de uma lista — por isso não há uma terceira.
#
# Saída: JSON de uma linha, pronto para `fromJSON` no `strategy.matrix`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib/tool_guard.sh
source "${SCRIPT_DIR}/lib/tool_guard.sh"

require_tool jq "membros do workspace para a matriz de testes" \
  "sudo apt-get install -y jq" || exit 1

# Crates sem nenhum alvo de teste, e o motivo. Qualquer outro crate sem testes
# reprova — que é o comportamento desejado.
#
# serverust-macros: proc-macro puro; os testes dele (trybuild + runtime) vivem
#   em serverust-macros-tests, que não pode ser dev-dep daqui sem recriar o
#   ciclo de dependências.
# hello-world: existe para medir cold start. Teste algum deve ser adicionado
#   aqui — o binário é o experimento.
SEM_TESTES=(
  "serverust-macros"
  "hello-world"
)

# Combinações de features além do default, uma linha por combinação.
COMBINACOES_EXTRA='[
  {"crate": "serverust-events", "features": "sqs in-memory"},
  {"crate": "serverust-events", "features": "kafka"},
  {"crate": "serverust-events", "features": "asyncapi sqs"}
]'

metadata="$(cargo metadata --no-deps --format-version 1)"

# `no_tests: pass` para os crates da lista; o resto reprova se ficar sem teste.
# `kafka: true` quando o crate depende de rdkafka — o job usa isso para
# instalar a toolchain de C. Derivado das deps reais: crate que passar a usar
# rdkafka amanhã não precisa de ninguém editando lista nenhuma.
base="$(
  jq -c --argjson sem_testes "$(printf '%s\n' "${SEM_TESTES[@]}" | jq -R . | jq -s .)" '
    [ .packages[]
      | {
          crate: .name,
          features: "",
          no_tests: (if (.name as $n | $sem_testes | index($n)) then "pass" else "" end),
          kafka: ([.dependencies[].name] | any(startswith("rdkafka")))
        }
    ]' <<<"$metadata"
)"

jq -c -n --argjson base "$base" --argjson extra "$COMBINACOES_EXTRA" '
  # As combinações extra herdam o `kafka` da linha base do mesmo crate, para
  # que a instalação da toolchain não dependa do nome da feature.
  ($base | map({key: .crate, value: .kafka}) | from_entries) as $kafka_por_crate
  # `no_tests: ""` também nas extras: linhas da matriz com o mesmo conjunto de
  # chaves evitam que uma chave ausente vire `null` numa expressão do Actions.
  | $base + ($extra | map({no_tests: ""} + . + {kafka: (($kafka_por_crate[.crate] // false) or (.features | test("kafka")))}))
  | {include: .}
'
