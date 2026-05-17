# ADR 0006 — rust-rdkafka como biblioteca Kafka para serverust-events

- **Status:** Accepted
- **Date:** 2026-05-16
- **Deciders:** maintainers serverust

---

## Contexto e Problema

`serverust-events` v0.2.0 adiciona suporte a Kafka com um broker bidirecional (`KafkaBroker`). Era necessário escolher a biblioteca Rust que implementaria o acesso ao protocolo Kafka, equilibrando maturidade, suporte a MSK IAM e impacto no binary size.

## Drivers de Decisão

- **Suporte a MSK IAM** — AWS Managed Streaming for Kafka usa SASL_SSL + OAUTHBEARER; é o ambiente-alvo de produção.
- **Maturidade e ecossistema** — a biblioteca precisa suportar Kafka 2.x/3.x, topics compactados, consumer groups e offsets.
- **Compile-time e binary size** — serverust prioriza cold start em Lambda; deps pesadas devem ficar atrás de feature flags.
- **Suporte a tokio** — o runtime de todo o framework é tokio; a biblioteca deve integrar sem workarounds.
- **Manutenção ativa** — frequência de releases, resposta a CVEs, compatibilidade com rdkafka C upstream.

## Opções Consideradas

### 1. `rust-rdkafka` (escolhida)

- Wrapper Rust sobre `librdkafka` (C). Requer compilação C (`cmake-build`).
- Ecossistema mais maduro do Rust para Kafka; usada em produção por Confluent, Materialize, Redpanda.
- Feature `sasl` usa Cyrus SASL (pesada, problemática em build local); MSK IAM usa OAUTHBEARER puro, sem Cyrus.
- Integração tokio via `FutureProducer` e `StreamConsumer`.
- Suporte a MSK IAM via `aws-msk-iam-sasl-signer` (callback OAUTHBEARER custom).

### 2. `RSKafka`

- Implementação nativa Rust do protocolo Kafka (sem librdkafka C).
- Binary size menor (sem librdkafka embutida ~600 KB).
- Não suporta SASL_SSL/OAUTHBEARER (sem MSK IAM out-of-box em 2026-05).
- API menos estável; menos usada em produção.
- Producer e consumer requerem mais configuração manual para consumer groups.

### 3. `kafka` (crate)

- Implementação Rust mais antiga; pouco mantida; sem suporte a Kafka 3.x features.
- Descartada por falta de manutenção.

## Decisão

Adotar **`rust-rdkafka`** atrás da feature opt-in `kafka` em `serverust-events`.

Configuração escolhida: `features = ["cmake-build", "tokio", "ssl"]` — **sem** `sasl` (evita Cyrus SASL). O OAUTHBEARER para MSK IAM é implementado via callback manual com `aws-msk-iam-sasl-signer`, sem dependência de Cyrus.

## Consequências

### Positivas
- MSK IAM funciona out-of-box em produção.
- Ecossistema maduro: suporte a Kafka 2.x/3.x, consumer groups, compacted topics.
- Integração tokio nativa via `FutureProducer` e `StreamConsumer`.

### Negativas
- Requer toolchain C no build (`cmake`, `clang`/`gcc`); aumenta tempo de CI em ~30-60s.
- `librdkafka` adiciona ~600 KB ao binário stripped quando a feature é ativa.
- A feature `kafka` **nunca deve entrar em `serverust-core`** (invariante de ADR 0003).

### Mitigações
- Feature `kafka` é opt-in; projetos HTTP-only não compilam rdkafka.
- `examples/hello-world` tem invariante explícita: `cargo tree -p hello-world | grep kafka` deve retornar vazio.
