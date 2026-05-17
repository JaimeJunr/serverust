# Tier List — Inspirações para a API Kafka do serverust-events

Frameworks e bibliotecas estudados durante o design da API event-driven (v0.2.0).

## S — Referências diretas (influência direta no design)

### Axum

- **O que copiamos:** padrão de extractors tipados (`FromRequest` → `FromExtractor`), turbofish com generics para handlers, composição via builder (`Router::new().route(...)` → `EventRouter::new().subscribe(...)`).
- **Diferença:** Axum usa HTTP request/response; serverust-events usa `BrokerMessage`/payload. O padrão de extração é análogo.

### AWS Lambda Powertools (Python)

- **O que copiamos:** decoradores de handler (`@event_source`, `@logger`) como inspiração para `#[subscriber]` / `#[publisher]`.
- **Diferença:** Python usa registro global implícito; serverust usa `register(router)` explícito para evitar singletons.

## A — Influência moderada

### Spring Kafka (`@KafkaListener`)

- Inspiração para a sintaxe `#[subscriber(topic = "...")]`.
- Spring usa container global de beans; preferimos builder explícito por testabilidade.

### Actix-web

- `App::new().data(state).service(...)` como modelo para `EventRouter::new().with_state(s).subscribe(...)`.
- Actix é actor-based; serverust é future-based — arquiteturas divergentes, mas o padrão de builder com estado é similar.

### Celery (Python)

- `@app.task` como inspiração para macro que converte função em unidade registrável.
- O conceito de "task" é análogo ao "subscriber".

## B — Referência pontual

### Kafka Streams (Java)

- API `builder.stream("topic").mapValues(...).to("output")` inspirou o pipeline `subscribe_publish` (input topic → transform → output topic).
- Streams é stateful e distributed; serverust é stateless por invocação — complexidade muito diferente.

### tokio-stream

- Uso de `StreamConsumer` do rdkafka como async iterator — padrão de consumo estudado mas encapsulado atrás de `Broker::subscribe` para não vazar rdkafka na API pública.

## C — Estudado, não adotado

### NATS.rs (`async-nats`)

- API limpa para publish/subscribe, mas NATS tem semântica diferente de Kafka (sem consumer groups, sem retention por offset).
- Estudado como referência de ergonomia; não influenciou o design diretamente.

### Lapin (RabbitMQ)

- API orientada a canais e exchanges; semântica muito diferente de Kafka topics.
- Confirmou a decisão de abstrair transporte via trait `Broker` para não vazar detalhes de protocolo.

### kafka-rs (crate descontinuada)

- API antiga, sem suporte a Kafka 2.x+.
- Descartada em favor de `rust-rdkafka` (ver ADR 0006).

## RSKafka (native Rust)

- Implementação nativa sem librdkafka.
- Sem suporte a MSK IAM SASL_SSL/OAUTHBEARER em 2026-05.
- Descartada para produção (ver ADR 0006); potencial alternativa futura para ambientes sem MSK.
