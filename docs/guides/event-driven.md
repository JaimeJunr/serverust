# Guia de Uso: Event-Driven com serverust-events

Este guia cobre as APIs event-driven do `serverust-events` introduzidas no v0.2.0, com exemplos de US-1 a US-7.

## Conceitos centrais

| Conceito | Tipo | Descrição |
|---|---|---|
| `Broker` | trait | Abstração de transporte: `subscribe` + `publish` |
| `EventRouter` | struct | Builder que compõe subscriptions e publica |
| `#[subscriber]` | macro | Declara um handler de eventos em uma função async |
| `#[publisher]` | macro | Empilhado em `#[subscriber]`, publica o valor de retorno |
| `LambdaBroker` | struct | Broker sink-only para modo AWS Lambda |
| `KafkaBroker` | struct (feat `kafka`) | Broker bidirecional via rust-rdkafka |
| `InMemoryBroker` | struct (feat `in-memory`) | Broker em memória para testes |

---

## US-1 — Trait `Broker` e `KafkaBroker`

A trait `Broker` define o contrato mínimo de qualquer transporte:

```rust
use std::sync::Arc;
use async_trait::async_trait;
use serverust_events::broker::{Broker, BrokerError, BrokerMessage, HandlerFuture};

#[async_trait]
impl Broker for MeuBroker {
    async fn subscribe(&self, topic: &str, handler: Arc<dyn Fn(BrokerMessage) -> HandlerFuture + Send + Sync>) -> Result<(), BrokerError> { ... }
    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), BrokerError> { ... }
}
```

`KafkaBroker` usa `rust-rdkafka` atrás da feature `kafka`:

```toml
serverust-events = { version = "0.2", features = ["kafka"] }
```

```rust
use serverust_events::broker::kafka::KafkaBroker;

let broker = KafkaBroker::from_env()?; // lê MSK_BOOTSTRAP_SERVERS / KAFKA_BROKERS
```

---

## US-2 — `InMemoryBroker` para testes

Sem Kafka rodando, use `InMemoryBroker` (feature `in-memory`):

```toml
[dev-dependencies]
serverust-events = { version = "0.2", features = ["in-memory"] }
```

```rust
use std::sync::Arc;
use serverust_events::broker::{Broker, in_memory::InMemoryBroker};

let broker = Arc::new(InMemoryBroker::new());
broker.publish("meu-topico", b"{\"id\":1}").await?;

let msgs = broker.messages("meu-topico");
assert_eq!(msgs.len(), 1);
```

---

## US-3 — `EventRouter` e builder programático

Registre handlers de forma fluente sem macros:

```rust
use std::sync::Arc;
use std::time::Duration;
use serverust_events::{router::EventRouter, retry::RetryPolicy};

EventRouter::new()
    .subscribe::<OrderCreated, _, _>("orders.created", handle_order)
    .with_retry(RetryPolicy::exponential(3, Duration::from_secs(1)))
    .with_dlq("orders.dlq")
    .attach(Arc::new(broker))
    .await?;
```

Para publicar o resultado em outro tópico:

```rust
EventRouter::new()
    .subscribe_publish::<WalletEvent, WalletResult, _, _>(
        "wallet.events",
        "wallet.results",
        handle_wallet,
    )
    .attach(broker)
    .await?;
```

---

## US-4 — Extractors tipados

Handlers podem declarar extractors como parâmetros adicionais via `subscribe_with`:

```rust
use serverust_events::extract::{EventCtx, KafkaHeaders, State};

EventRouter::new()
    .with_state(AppState { db: pool })
    .subscribe_with("orders.created", |event: OrderCreated, ctx: EventCtx, State(app): State<AppState>| async move {
        println!("offset={} topic={}", ctx.offset.unwrap_or(0), ctx.topic);
        // usa app.db ...
        Ok(())
    })
    .attach(broker)
    .await?;
```

Extractors disponíveis:

| Extractor | Dado exposto |
|---|---|
| `EventCtx` | topic, partition, offset, timestamp |
| `KafkaHeaders` | headers como `HashMap<String, Vec<u8>>` |
| `State<S>` | estado compartilhado injetado via `with_state` |

---

## US-5 — `RetryPolicy`

```rust
use serverust_events::retry::RetryPolicy;
use std::time::Duration;

// Retenta 3x imediatamente
RetryPolicy::immediate(3)

// Backoff exponencial: 1s, 2s, 4s
RetryPolicy::exponential(3, Duration::from_secs(1))
```

Encadeie na subscrição:

```rust
EventRouter::new()
    .subscribe::<T, _, _>("topico", handler)
    .with_retry(RetryPolicy::exponential(3, Duration::from_secs(1)))
    .with_dlq("topico.dlq")
```

---

## US-6 — Macros `#[subscriber]` e `#[publisher]`

As macros eliminam o boilerplate do builder para handlers estáticos:

```rust
use serverust_macros::{subscriber, publisher};
use serverust_events::broker::BrokerError;

// Handler ack-only (não publica)
#[subscriber(topic = "orders.created")]
async fn handle_order(event: OrderCreated) -> Result<(), BrokerError> {
    // processa...
    Ok(())
}

// Handler com pipeline de publicação
#[subscriber(topic = "orders.created")]
#[publisher(topic = "orders.confirmed")]
async fn process_and_confirm(event: OrderCreated) -> Result<OrderConfirmed, BrokerError> {
    Ok(OrderConfirmed { id: event.id })
}
```

O código gerado cria uma struct unit homônima com:

- `handle_order::SUBSCRIBE_TOPIC` — tópico de entrada
- `handle_order::PUBLISH_TOPIC` — `Option<&'static str>` com tópico de saída
- `handle_order::register(router)` — registra no `EventRouter`

Composição de múltiplos handlers:

```rust
let router = handle_order::register(EventRouter::new());
let router = process_and_confirm::register(router);
router.attach(broker).await?;
```

### Nota sobre `#[publisher]` em modo Lambda

`LambdaBroker` é sink-only (recebe, não publica). O `#[publisher]` funciona end-to-end com `KafkaBroker` (long-running) ou `InMemoryBroker` (testes). Em Lambda, use um `KafkaProducer` separado para publicar resultados.

---

## US-7 — Detecção de runtime

`Runtime::detect()` inspeciona `AWS_LAMBDA_FUNCTION_NAME`:

```rust
use serverust_events::runtime::Runtime;
use serverust_events::broker::lambda::LambdaBroker;
use lambda_runtime::{service_fn, LambdaEvent};

let router = handle_wallet::register(EventRouter::new());

match Runtime::detect() {
    Runtime::Lambda => {
        let broker = Arc::new(LambdaBroker::new());
        router.attach(broker.clone()).await?;
        lambda_runtime::run(service_fn(move |event: LambdaEvent<KafkaEvent>| {
            let broker = broker.clone();
            async move {
                broker.handle_kafka_event(&event.payload).await
                    .map_err(|e| e.to_string())
            }
        }))
        .await?;
    }
    Runtime::LongRunning => {
        // usa KafkaBroker::from_env() + loop de dispatch
    }
}
```

---

## Exemplo completo: kafka-wallet

Veja [`examples/kafka-wallet/`](../../examples/kafka-wallet/) para o exemplo end-to-end
**wallet.events → DynamoDB → wallet.results** usando `#[subscriber]` + `#[publisher]` + `EventRouter`.
