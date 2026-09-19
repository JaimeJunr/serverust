#!/usr/bin/env bash
# Guard de presença de ferramenta externa para os gates de qualidade.
# Arquivo de biblioteca: destinado a `source`, não a execução direta.
#
# O padrão que isto substitui saía 0 quando a ferramenta não estava instalada,
# o que torna "não verifiquei" indistinguível de "verifiquei e passou" — o
# oposto do default que falha fechado exigido por docs/product/philosophy.md
# ("Falhe fechado": sob default permissivo a omissão é invisível).
#
# São TRÊS estados, nunca dois:
#   ferramenta presente e checagem passou -> 0, silencioso
#   ferramenta presente e checagem falhou -> != 0 (a própria ferramenta decide)
#   ferramenta AUSENTE                    -> aviso alto e inequívoco em stderr,
#                                            != 0 em CI, 0 localmente
# O desenvolvedor local não fica travado por falta de ferramenta, mas o CI nunca
# reporta verde sem ter verificado.

# require_tool <binário> <o que deixou de ser verificado> <como instalar>
# 0 = ferramenta presente, siga em frente.
# 1 = ferramenta ausente fora de CI; o chamador deve encerrar (`|| exit 0`).
# Em CI a função não retorna: encerra o gate com exit 1.
require_tool() {
  local bin="$1" unchecked="$2" install="$3"

  if command -v "$bin" >/dev/null 2>&1; then
    return 0
  fi

  {
    echo "=================================================================="
    echo "PULADO: ${bin} não instalado — NENHUMA verificação de ${unchecked}"
    echo "        foi feita. Este gate NÃO atesta nada nesta execução."
    echo "        Instale com: ${install}"
    echo "=================================================================="
  } >&2

  if [[ -n "${CI:-}" ]]; then
    echo "CI detectado: gate reprovado em vez de passar sem verificar." >&2
    exit 1
  fi

  return 1
}
