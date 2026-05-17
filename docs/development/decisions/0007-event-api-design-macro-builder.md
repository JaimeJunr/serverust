# ADR 0007 — Design da API event-driven: macro + builder programático

- **Status:** Accepted
- **Date:** 2026-05-16
- **Deciders:** maintainers serverust

---

## Contexto e Problema

`serverust-events` v0.2.0 precisava de uma API para registrar handlers de eventos Kafka. O design precisava equilibrar ergonomia para o caso comum, flexibilidade para casos avançados (handlers dinâmicos, condicionais) e alinhamento com os padrões já estabelecidos em `serverust-core` (macros de atributo + DI via extractors).

## Drivers de Decisão

- **DX consistente** — a camada HTTP do serverust usa macros (`#[get]`, `#[post]`) + extractors tipados; a camada event-driven deve seguir o mesmo padrão.
- **Sem registro global mágico** — macros devem gerar código estático e composável, não registro em singletons ocultos (dificulta testes e tree-shaking).
- **Flexibilidade para casos avançados** — builders programáticos permitem handlers dinâmicos que macros não cobrem.
- **Testabilidade** — testes de unidade devem rodar sem Kafka físico.
- **Composição** — múltiplos handlers devem ser combinados sem conflito.

## Opções Consideradas

### 1. Apenas macros de atributo (sem builder)

- DX excelente para o caso comum.
- Inflexível: handlers condicionais ou dinâmicos são impossíveis.
- Registro global mágico seria necessário para inicialização.

### 2. Apenas builder programático (sem macros)

- Máxima flexibilidade.
- Verboso para o caso comum: muito boilerplate para handlers simples.
- Não aproveita o padrão já estabelecido em `serverust-core`.

### 3. Macro + builder (escolhida)

- `#[subscriber]` / `#[publisher]` para o caso comum.
- `EventRouter::subscribe` / `subscribe_publish` / `subscribe_with` para casos avançados.
- Macros geram código que *usa o builder* internamente — não há divergência de comportamento.
- Composição explícita: `handle_a::register(EventRouter::new())` + `handle_b::register(router)`.

## Decisão

Adotar a API dual **macro + builder**:

### Macros geradas

`#[subscriber(topic = "...")]` transforma uma `async fn` em uma struct unit com:

```rust
pub struct handle_order;
impl handle_order {
    pub const SUBSCRIBE_TOPIC: &'static str = "orders.created";
    pub const PUBLISH_TOPIC: Option<&'static str> = None;
    pub fn register(router: EventRouter) -> EventRouter { ... }
}
```

`#[publisher(topic = "...")]` empilhado ativa `subscribe_publish` com o tópico de saída.

### Builder programático

`EventRouter` expõe:
- `subscribe::<T, H, Fut>(topic, handler)` — ack-only
- `subscribe_publish::<T, U, H, Fut>(sub, pub, handler)` — pipeline
- `subscribe_with::<T, Exts, H>(topic, handler)` — com extractors (State, EventCtx, KafkaHeaders)
- `with_retry(policy)` / `with_dlq(topic)` — políticas por subscription
- `with_state(s)` — estado compartilhado
- `attach(broker)` — finaliza e registra no broker

### Inspirações

- **Axum** — turbofish `subscribe::<T, _, _>`, padrão de extractors tipados, service composability.
- **Actix-web** — `App::new().service(...)` como modelo de builder com estado compartilhado.
- **AWS Lambda Powertools (Python)** — decorators para handlers de eventos (equivalente às macros).
- **Spring Kafka** — `@KafkaListener` como inspiração para `#[subscriber]`.

Ver ranking completo em [`docs/research/kafka-inspiration-tier-list.md`](../../research/kafka-inspiration-tier-list.md).

## Consequências

### Positivas
- DX consistente com `serverust-core` (macros + extractors).
- Testes triviais: `InMemoryBroker` funciona identicamente a `KafkaBroker` pelo polimorfismo da trait `Broker`.
- Macros não introduzem registro global — cada handler tem `register(router)` explícito.
- Builder cobre 100% dos casos que macros não cobrem.

### Negativas
- `subscribe_publish` depende de um broker bidirecional; `LambdaBroker` é sink-only — padrão não funciona end-to-end em Lambda sem KafkaBroker ou producer separado.
- Turbofish `subscribe::<T, _, _>` com 3 generics é necessário por limitação de inferência do compilador Rust (handler `H` + future `Fut` ambos precisam ser inferidos).

### Decisão de design: sem registro global

Macros em outros frameworks (Spring, Quarkus) tipicamente registram em um container global. Aqui optamos por `register(router)` explícito por dois motivos:
1. **Testabilidade** — cada teste cria seu próprio `EventRouter` sem poluição de estado global.
2. **Tree-shaking** — handlers não utilizados não são compilados (sem `inventory` ou `linkme`).
