# Guia de Uso: Event-Driven com serverust-events

Este guia cobre as APIs event-driven do `serverust-events`: **Kafka** (v0.2.0) e **SQS** (v0.3.0). O crate permanece opt-in — `serverust-core` e `hello-world` não puxam Kafka nem SQS por padrão.

## Conceitos centrais

| Conceito | Tipo | Descrição |
|---|---|---|
| `Broker` | trait | Abstração de transporte: `subscribe` + `publish` |
| `EventRouter` | struct | Builder que compõe subscriptions e publica |
| `#[subscriber]` | macro | Handler de eventos; `driver = "kafka"` ou `driver = "sqs"` |
| `#[publisher]` | macro | Empilhado em `#[subscriber]`, publica o valor de retorno |
| `LambdaBroker` | struct | Broker sink-only para trigger MSK em Lambda |
| `KafkaBroker` | struct (feat `kafka`) | Broker bidirecional via rust-rdkafka |
| `SqsBroker` | struct (feat `sqs`) | Broker sink-only para SQS em Lambda ESM |
| `StandaloneSqsBroker` | struct (feat `sqs`) | Long-poll worker para ECS/EC2 |
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
serverust-events = { version = "0.3", features = ["kafka"] }
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
serverust-events = { version = "0.3", features = ["in-memory"] }
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

**Comportamento do backoff:** cada atraso é `base_delay * 2^n` com `n` limitado a 31 e multiplicação saturante em [`Duration`](https://doc.rust-lang.org/std/time/struct.Duration.html) — o produto nunca faz panic por overflow; valores acima do máximo de `Duration` saturam em `Duration::MAX`.

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

---

## v0.3 — SQS (feature `sqs`)

A feature `sqs` ativa o módulo `serverust_events::sqs` e `aws_lambda_events/sqs`. Não adiciona dependências C — apenas deserialização de `SqsEvent`/`SqsBatchResponse` e pipeline Tower (idempotency, retry, métricas EMF).

```toml
serverust-events = { version = "0.3", features = ["sqs"] }
# Testes locais sem AWS:
# serverust-events = { version = "0.3", features = ["sqs", "in-memory"] }
```

### Macro `#[subscriber(driver = "sqs", queue = "...")]`

A mesma macro usada para Kafka aceita `driver = "sqs"` e roteia pelo **nome da fila** (segmento final do ARN em Lambda, ou tópico lógico no `InMemoryBroker` em testes):

```rust
use serverust_macros::subscriber;
use serverust_events::broker::BrokerError;

#[subscriber(driver = "sqs", queue = "orders")]
async fn handle_order(event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}
```

Constantes geradas (úteis em testes e no CLI):

| Constante | Significado |
|---|---|
| `handle_order::SUBSCRIBE_TOPIC` | Nome da fila (`"orders"`) |
| `handle_order::DRIVER` | `"sqs"` ou `"kafka"` |
| `handle_order::register(router)` | Inscreve no `EventRouter` |

`topic = "..."` sem `driver` continua sendo **Kafka** (compatibilidade v0.2).

Retry e DLQ declarativos na macro (US-008):

```rust
#[subscriber(
    driver = "sqs",
    queue = "orders",
    retry = exponential(max = 5, base = "100ms"),
    dlq = "orders-dlq"
)]
async fn process_order(event: OrderCreated) -> Result<(), BrokerError> { ... }
```

Filas **FIFO** exigem `fifo` na macro e o extractor `SqsFifoMetadata` no handler:

```rust
use serverust_events::sqs::extract::SqsFifoMetadata;

#[subscriber(driver = "sqs", queue = "orders.fifo", fifo)]
async fn handle_fifo(event: OrderCreated, meta: SqsFifoMetadata) -> Result<(), BrokerError> {
    let _group = meta.message_group_id;
    Ok(())
}
```

### Lambda ESM — `SqsBroker`

Em Lambda com event source mapping SQS, o runtime AWS entrega um `SqsEvent` por invocação. O `SqsBroker` é **sink-only**: não faz `ReceiveMessage`; despacha registros e devolve `SqsBatchResponse` com `batch_item_failures` para partial batch failure (`ReportBatchItemFailures`).

```rust
use std::sync::Arc;
use aws_lambda_events::event::sqs::SqsEvent;
use lambda_runtime::{service_fn, LambdaEvent};
use serverust_events::router::EventRouter;
use serverust_events::sqs::consumer::SqsBroker;

let broker = Arc::new(SqsBroker::new());
handle_order::register(EventRouter::new())
    .attach(broker.clone())
    .await?;

lambda_runtime::run(service_fn(move |event: LambdaEvent<SqsEvent>| {
    let broker = broker.clone();
    async move { Ok(broker.handle_sqs_event(&event.payload).await) }
}))
.await?;
```

**Routing:** a fila é o último segmento de `event_source_arn` (`arn:aws:sqs:region:account:queue-name`). O handler deve estar inscrito nesse nome via `queue = "..."` na macro.

**Invariante (v0.3+):** mensagens sem handler para a fila do ARN, ou sem ARN válido, entram em `batch_item_failures` quando há `message_id` — evita ack silencioso pela Lambda. Sem `message_id`, apenas log (`tracing::warn`); o batch inteiro pode ser retentado.

### Standalone — `StandaloneSqsBroker`

Em ECS/EC2, use long-poll com traits mockáveis `ReceiveClient` e `DeleteClient` (integração típica com `aws-sdk-sqs`):

```rust
use serverust_events::sqs::standalone::{StandaloneSqsBroker, StandaloneConfig};

let broker = Arc::new(StandaloneSqsBroker::new(
    receive_client,
    delete_client,
    queue_url,
    "orders".into(), // nome lógico = SUBSCRIBE_TOPIC
));

handle_order::register(EventRouter::new())
    .attach(broker.clone())
    .await?;

// SIGTERM → broker.signal_shutdown(); depois:
broker.run().await?;
```

Mesmos handlers `#[subscriber(driver = "sqs")]` funcionam em Lambda ESM e standalone. Ack no standalone: `DeleteMessageBatch` após sucesso; em Lambda ESM o ack é via ausência em `batch_item_failures`.

### Extractors SQS

| Extractor | Dado exposto |
|---|---|
| `Json<T>` | Body deserializado (serde) |
| `SqsMetadata` | `message_id`, `receipt_handle`, attributes |
| `SqsFifoMetadata` | `message_group_id`, deduplication, sequence (FIFO) |
| `State<S>` | Estado compartilhado via `EventRouter::with_state` |

`SqsMetadata` exige que a mensagem tenha passado por `SqsBroker` ou `StandaloneSqsBroker` (header interno `__serverust_sqs_message`).

### Publicação — `SqsProducer`

`SqsBroker::publish` retorna erro — use `serverust_events::sqs::producer::SqsProducer` para envio com batching (até 10 msgs / linger configurável) e retry em partial failure. FIFO: `FifoSendBuilder` type-state — `send()` só compila após `.message_group_id(...)`.

### Pipeline Tower (opcional em handlers avançados)

`SqsSubscriber` implementa `tower::Service<SqsMessage>` com camadas: tracing → métricas EMF → idempotency (`IdempotencyStore`, DynamoDB opcional) → retry → handler. Heartbeat (`ChangeMessageVisibility`) ativo por padrão no standalone; opt-in em Lambda ESM (`serverust_events::sqs::heartbeat`).

### Transport abstraction (Kafka ↔ SQS)

`EventRouter::attach` aceita qualquer `impl Broker`. O mesmo handler pode ser testado com `InMemoryBroker` e implantado com `SqsBroker` ou `KafkaBroker`. Testes de paridade: `serverust-events/tests/transport_parity.rs`.

### CLI operacional

```bash
serverust queue inspect <queue-url>   # atributos da fila (IAM na conta AWS)
serverust queue tail <queue-url>      # amostra de mensagens (--max N)
```

Requer credenciais AWS (env vars ou role). Não substitui o consumer — útil para troubleshooting em dev/staging.

### Troubleshooting SQS

| Sintoma | Causa provável | Ação |
|---|---|---|
| Mensagem some sem processar | Handler não registrado para o nome da fila do ARN | Conferir `queue =` vs ARN; ver logs `sqs record with no handler` |
| Batch inteiro retenta | Erro sem `message_id` reportável | Garantir `ReportBatchItemFailures` + IDs nas mensagens |
| FIFO falha em runtime | Handler sem `SqsFifoMetadata` com `fifo` | Adicionar macro `fifo` + extractor |
| Idempotency não deduplica | `message_id` vazio | Log `idempotency bypass`; corrigir produtor |
| Cold start regressou | Feature `sqs` no binário mínimo | Manter `hello-world` sem feature `sqs` |

Testes de referência (comportamento verificável): `serverust-events/tests/macros_sqs*.rs`, `sqs_consumer.rs`, `sqs_fifo.rs`, `sqs_idempotency.rs`, `sqs_dlq.rs`.

Pesquisa de design: [`docs/research/sqs-inspiration-tier-list.md`](../research/sqs-inspiration-tier-list.md).
