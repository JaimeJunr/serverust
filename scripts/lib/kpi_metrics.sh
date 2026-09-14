#!/usr/bin/env bash
# Lógica compartilhada dos gates de KPI (quality_kpi_gate.sh,
# metrics_regression_check.sh, benchmark_ci.sh, metrics_append.sh).
# Arquivo de biblioteca: destinado a `source`, não a execução direta.

# Número de amostras de startup coletadas por medição. Amostra única deixava o
# baseline refém de onde ele caiu na distribuição de ruído de agendamento do SO.
STARTUP_SAMPLES="${STARTUP_SAMPLES:-5}"

# Teto absoluto do eixo de tempo, em ms. O startup local NÃO é comparado contra
# o baseline: medições isoladas da mesma build, sem nenhuma alteração de código,
# variaram de 13ms a 62ms na mesma máquina (ver ADR 0008). Comparar contra o
# baseline nessa dispersão reprova ruído. O que este teto pega é regressão
# grosseira — uma dependência pesada entrando no caminho de inicialização.
STARTUP_ABSOLUTE_MAX_MS="${STARTUP_ABSOLUTE_MAX_MS:-2000}"

# Eixo determinístico: mesmo código produz byte-idêntico, então piso zero.
BYTES_TOLERANCE_PCT="${BYTES_TOLERANCE_PCT:-5}"
BYTES_TOLERANCE_FLOOR="${BYTES_TOLERANCE_FLOOR:-0}"

# exceeds_absolute <valor> <teto>
# Imprime "yes" se o valor ultrapassa o teto absoluto.
exceeds_absolute() {
  awk -v v="$1" -v m="$2" 'BEGIN { if (v > m) print "yes"; else print "no" }'
}

# exceeds_tolerance <baseline> <atual> <pct> [piso_absoluto]
# Imprime "yes" se o valor atual excede a tolerância, "no" caso contrário.
# Tolerância = max(baseline * pct / 100, piso_absoluto).
exceeds_tolerance() {
  local baseline="$1" current="$2" pct="$3" floor="${4:-0}"
  awk -v b="$baseline" -v c="$current" -v p="$pct" -v f="$floor" 'BEGIN {
    margin = b * p / 100
    if (margin < f) margin = f
    if (c > b + margin) print "yes"; else print "no"
  }'
}

# median_of <n1> [n2 ...]
# Mediana das amostras. Em contagem par usa o elemento inferior do meio, para
# manter o resultado inteiro em ms (sem meio milissegundo artificial).
median_of() {
  printf '%s\n' "$@" | sort -n | awk '{ v[NR] = $1 } END {
    if (NR == 0) exit 1
    print v[int((NR + 1) / 2)]
  }'
}

# measure_startup_once <binário> <porta>
# Sobe o binário, cronometra até a primeira resposta HTTP e o derruba.
# Imprime o tempo em ms, ou falha (exit 1) se o servidor não subir.
measure_startup_once() {
  local bin="$1" port="$2"
  local pid start_ms end_ms

  "$bin" >/tmp/serverust_startup_stdout.log 2>/tmp/serverust_startup_stderr.log &
  pid=$!

  start_ms="$(date +%s%3N)"
  for _ in $(seq 1 100); do
    if curl -sf "http://127.0.0.1:${port}/" >/dev/null 2>&1; then
      end_ms="$(date +%s%3N)"
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" 2>/dev/null || true
      echo $((end_ms - start_ms))
      return 0
    fi
    sleep 0.05
  done

  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" 2>/dev/null || true
  return 1
}

# measure_startup_median_ms <binario> <porta> [amostras]
# Mediana de N medições, impressa em ms. As amostras brutas vão para
# STARTUP_SAMPLES_FILE — a função roda sob `$(...)`, então variável não volta
# para o chamador; arquivo volta.
STARTUP_SAMPLES_FILE="${STARTUP_SAMPLES_FILE:-/tmp/serverust_startup_samples}"
measure_startup_median_ms() {
  local bin="$1" port="$2" samples="${3:-$STARTUP_SAMPLES}"
  local collected=() value

  for _ in $(seq 1 "$samples"); do
    if ! value="$(measure_startup_once "$bin" "$port")"; then
      continue
    fi
    collected+=("$value")
    # A porta precisa ser liberada pelo kernel antes da próxima amostra.
    sleep 0.2
  done

  if [ "${#collected[@]}" -eq 0 ]; then
    return 1
  fi

  printf '%s\n' "${collected[*]}" > "$STARTUP_SAMPLES_FILE"
  median_of "${collected[@]}"
}
