# ADR 0008 — Startup local vira eixo informativo; gate estrito fica no binário

- **Status:** Accepted
- **Date:** 2026-09-13
- **Deciders:** maintainers serverust

---

## Contexto e Problema

`scripts/quality_kpi_gate.sh` reprovava de forma não determinística no eixo de tempo. Com o baseline de 11 ms registrado para a v0.4.1 em `docs/product/metrics/history.json` e tolerância de 20%, o limite ficava em 13,2 ms. Três execuções seguidas, sem nenhuma alteração de código entre elas, mediram 13 ms (passou), 16 ms (falhou) e 14 ms (falhou).

Que não se trata de regressão real está evidenciado pelo outro eixo: nas mesmas execuções o `stripped_bytes` medido foi byte-idêntico ao baseline. Só o tempo oscila.

O diagnóstico inicial apontava duas causas: tolerância percentual sobre um valor absoluto pequeno (20% de 11 ms são 2,2 ms) e baseline gravado a partir de uma única medição por `scripts/metrics_append.sh`.

**Medir antes de decidir mudou a conclusão.** Instrumentando a mesma build, sem nenhuma alteração de código:

| Condição | Amostras (ms) |
|---|---|
| Cinco medições em rajada (0,2 s entre elas) | 28, 21, 24, 62, 20 |
| Cinco medições em rajada, repetição | 30, 27, 16, 25, 18 |
| Cinco medições isoladas (2 s entre elas) | 24, 20, 28, 21, 35 |
| Primeira medição logo após o build | 13 |

A dispersão é de ~5x (13 ms a 62 ms) e **não** melhora ao espaçar as medições — isoladas e em rajada produzem a mesma faixa. Ela vem do estado da máquina, não da cadência de amostragem nem do código: repetida a mesma medição com a máquina ociosa, a faixa cai para 11–15 ms. O que este eixo mede, nesta escala, é o quanto a máquina de quem commita está ocupada.

Isso invalida a correção que parecia óbvia. Um piso de 5 ms sobre um baseline de 11 ms dá limite de 16 ms — abaixo da mediana observada na maioria das execuções acima. O piso apenas adiaria a próxima reprovação aleatória.

Há ainda uma divergência entre nome e medição: o campo se chamava `cold_start_p95_ms`, mas `scripts/benchmark_ci.sh` mede o tempo até a primeira resposta HTTP de um binário rodando localmente — não cold start de Lambda, e não um p95.

O gate é opt-in (`LEFTHOOK_KPI=1`) e portanto não bloqueava ninguém. O custo não era o bloqueio: um gate que reprova aleatoriamente ensina o time a ignorá-lo.

## Drivers de Decisão

- **Um gate só vale se seu verde significa algo.** Reprovação aleatória destrói isso mais rápido que ausência de gate.
- **Não abandonar o sinal de tempo por completo** — uma dependência pesada entrando no caminho de inicialização deve ser percebida.
- **Honestidade do dado.** O nome do campo deve descrever o que foi medido; o invariante público não deve parecer coberto por uma medição que não o cobre.
- **Preservar o eixo determinístico.** `stripped_bytes` detecta regressão de verdade e não precisa de folga adicional.

## Opções Consideradas

### 1. Piso absoluto além do percentual — `max(20%, 5 ms)` (rejeitada)

Foi a primeira implementação. Rejeitada pela medição acima: com dispersão de 13 ms a 62 ms, um limite de 16 ms continua dentro do ruído. Piso maior (30 ms? 60 ms?) seria arbitrário e, na prática, equivalente a desligar a comparação — só que fingindo que ela existe.

### 2. Medir N vezes e agregar (adotada como complemento)

Coletar 5 amostras e gravar a agregação em vez de uma medição única. Sozinha não resolve — a mediana de 5 amostras oscilou entre 21 ms e 25 ms nas execuções acima, contra um baseline de 11 ms. Mas melhora o que fica registrado em `history.json`: o valor deixa de depender de onde caiu a única amostra.

### 3. Separar os eixos (adotada)

Gate estrito onde a medição é determinística (`stripped_bytes`); startup local como informação registrada e reportada, não como critério de reprovação. É o que a evidência sustenta.

## Decisão

**Opção 3, com a 2 como complemento.**

- **`stripped_bytes` continua gate estrito:** 5% sobre a última entry, sem piso. Eixo determinístico.
- **`startup_local_p50_ms` passa a informativo.** O gate mede, reporta as amostras e o delta vs baseline, e **não reprova** por esse eixo.
- **Guarda de sanidade absoluta no lugar da comparação relativa:** reprova apenas acima de `STARTUP_ABSOLUTE_MAX_MS` (2000 ms, o mesmo teto que `benchmark_ci.sh` já aplicava). Toda a faixa de ruído observada passa; uma regressão grosseira no caminho de inicialização não passa.
- **Amostragem:** `benchmark_ci.sh` e `quality_kpi_gate.sh` coletam `STARTUP_SAMPLES` medições (default 5) e registram a **mediana** — em contagem par, o elemento inferior do meio, para manter o resultado inteiro em ms. Com 5 amostras o p95 é efetivamente o máximo, justamente a estatística mais contaminada pelo ruído.
- **Nomes:** o campo medido passa a ser `startup_local_p50_ms`, acompanhado de `startup_local_samples`. `cold_start_p95_ms` permanece no schema como o campo do invariante público — cold start p95 no Lambda ARM64 128 MB — e fica `null` enquanto não houver invocação real na AWS.
- Lógica compartilhada em `scripts/lib/kpi_metrics.sh`, coberta por `scripts/test_kpi_metrics_lib.sh`, que usa as medições reais desta ADR como casos que não devem reprovar.

Entradas históricas de `history.json` tiveram o rótulo corrigido: o valor sempre foi startup local, então migrou para `startup_local_p50_ms` com `startup_local_samples: 1`. Os leitores aceitam `cold_start_p95_ms` como fallback, para não quebrar entradas geradas por versões anteriores do script.

## Consequências

**Positivas**

- O gate para de reprovar execuções idênticas. Verde volta a significar "nada regrediu".
- `history.json` deixa de afirmar um cold start de Lambda que nunca foi medido.
- O valor registrado por release reflete a tendência central de 5 medições, não uma amostra sorteada.

**Negativas**

- Regressão de startup abaixo de 2000 ms não é pega automaticamente. É o preço de não ter uma medição confiável nessa escala: o número continua registrado em `history.json` a cada release, disponível para inspeção por tendência ao longo de várias versões — que é o horizonte em que esse dado tem significado.
- `benchmark_ci.sh` e o gate ficam ~5x mais lentos no trecho de startup. Alguns segundos.
- O invariante público de cold start em Lambda continua sem medição automatizada. Esta ADR não o resolve — apenas para de fingir que o resolve. Medi-lo exige `cargo-lambda` e invocação real em AWS no CI de release, e é o caminho para o eixo de tempo voltar a ser gate: lá a medição é do ambiente-alvo e menos sujeita ao ruído da máquina de quem commita.
