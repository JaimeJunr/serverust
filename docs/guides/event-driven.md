# Guia de Uso: Event-Driven com serverust-events

Este guia cobre as APIs event-driven opt-in do `serverust-events`: Kafka desde v0.2.0 e SQS desde v0.3.0. O objetivo é manter `serverust-core` leve: transportes concretos entram por features explícitas.

## Quando usar

| Caso | Recomendação |
|---|---|
| Consumer Kafka long-running | `KafkaBroker` com feature `kafka` |
| Trigger Kafka/MSK em Lambda | `LambdaBroker` e `aws_lambda_events::KafkaEvent` |
| Trigger SQS em Lambda | `SqsBroker` com feature `sqs` |
| Worker SQS em ECS/EC2/bare-metal | `StandaloneSqsBroker` com feature `sqs` |
| Testes sem broker físico | `InMemoryBroker` com feature `in-memory` |
| Contrato de eventos | `AsyncApiBuilder` ou `#[subscriber(..., asyncapi)]` com feature `asyncapi` |

## Dependências

Habilite somente o transporte usado pela aplicação:

```toml
[dependencies]
serverust-events = { version = "0.3", features = ["sqs", "asyncapi"] }
serverust-macros = "0.3"

# Kafka long-running:
# serverust-events = { version = "0.3", features = ["kafka"] }

[dev-dependencies]
serverust-events = { version = "0.3", features = ["in-memory"] }
```

Constraints importantes:

- `sqs`, `kafka`, `asyncapi` e `in-memory` são opt-in.
- `serverust-core` não depende de SQS/Kafka.
- `SqsBroker` em Lambda é sink-only: ele consome o evento entregue pela Lambda, mas não publica mensagens; para publicar use `SqsProducer`.
- `#[subscriber(topic = "...")]` continua sendo Kafka por compatibilidade. Para SQS, use `driver = "sqs"` e `queue = "..."`.

## Conceitos centrais

| Conceito | Tipo | Descrição |
|---|---|---|
| `Broker` | trait | Abstração de transporte: `subscribe` + `publish` |
| `EventRouter` | struct | Builder que compõe subscriptions, retry/DLQ programáticos e estado |
| `#[subscriber]` | macro | Declara um handler de eventos em uma função async |
| `#[publisher]` | macro | Empilhado em `#[subscriber]`, publica o valor de retorno quando o broker suporta publish |
| `KafkaBroker` | struct (feat `kafka`) | Broker bidirecional via rust-rdkafka |
| `LambdaBroker` | struct | Dispatch de eventos Kafka/MSK recebidos por Lambda |
| `SqsBroker` | struct (feat `sqs`) | Consumer SQS para Lambda Event Source Mapping |
| `StandaloneSqsBroker` | struct (feat `sqs`) | Worker SQS long-poll fora da Lambda |
| `SqsProducer` / `SqsFifoProducer` | structs (feat `sqs`) | Producers SQS com batching, retry e FIFO type-safe |

## API comum: `Broker` e `EventRouter`

A trait `Broker` define o contrato mínimo de qualquer transporte:

```rust
use async_trait::async_trait;
use serverust_events::broker::{BoxedHandler, Broker, BrokerError};

#[async_trait]
impl Broker for MeuBroker {
    async fn subscribe(&self, topic: &str, handler: BoxedHandler) -> Result<(), BrokerError> {
        // registra handler para a chave física do transporte
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), BrokerError> {
        // envia bytes para o tópico/fila
        Ok(())
    }
}
```

Registre handlers de forma fluente sem macros:

```rust
use std::time::Duration;
use serverust_events::{retry::RetryPolicy, router::EventRouter};

EventRouter::new()
    .subscribe::<OrderCreated, _, _>("orders.created", handle_order)
    .with_retry(RetryPolicy::exponential(3, Duration::from_secs(1)))
    .with_dlq("orders.dlq")
    .attach(broker)
    .await?;
```

Para publicar o retorno em outro canal:

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

## Macros `#[subscriber]` e `#[publisher]`

As macros eliminam boilerplate do builder para handlers estáticos:

```rust
use serverust_events::broker::BrokerError;
use serverust_macros::{publisher, subscriber};

#[subscriber(topic = "orders.created")]
async fn handle_kafka(event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}

#[subscriber(driver = "sqs", queue = "orders")]
async fn handle_sqs(event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}

#[subscriber(topic = "wallet.events")]
#[publisher(topic = "wallet.results")]
async fn process_and_publish(event: WalletEvent) -> Result<WalletResult, BrokerError> {
    Ok(WalletResult { id: event.id })
}
```

O código gerado expõe constantes e helpers no item homônimo:

- `handle_sqs::DRIVER`
- `handle_sqs::SUBSCRIBE_TOPIC`
- `handle_sqs::PUBLISH_TOPIC`
- `handle_sqs::RETRY_MAX_ATTEMPTS`
- `handle_sqs::DLQ_QUEUE`
- `handle_sqs::register(router)`

Composição de múltiplos handlers:

```rust
let router = handle_kafka::register(EventRouter::new());
let router = handle_sqs::register(router);
router.attach(broker).await?;
```

## Extractors tipados

Handlers podem declarar extractors adicionais via `subscribe_with` ou macros:

```rust
use serverust_events::extract::{EventCtx, KafkaHeaders, State};

EventRouter::new()
    .with_state(AppState { db: pool })
    .subscribe_with(
        "orders.created",
        |event: OrderCreated, ctx: EventCtx, State(app): State<AppState>| async move {
            println!("topic={}", ctx.topic);
            Ok(())
        },
    )
    .attach(broker)
    .await?;
```

| Extractor | Dado exposto |
|---|---|
| `EventCtx` | topic, partition, offset, timestamp |
| `KafkaHeaders` | headers como `HashMap<String, Vec<u8>>` |
| `State<S>` | estado compartilhado registrado com `with_state` |
| `Json<T>` | payload JSON explícito como extractor |
| `SqsMetadata` | `message_id`, `receipt_handle`, system attributes e message attributes |
| `SqsFifoMetadata` | `message_group_id`, `message_deduplication_id`, `sequence_number` |

## SQS em Lambda Event Source Mapping

`SqsBroker` consome `aws_lambda_events::event::sqs::SqsEvent` entregue pela Lambda. Ele roteia cada registro pelo nome da fila extraído de `event_source_arn` e retorna `SqsBatchResponse` com `batch_item_failures`.

```rust
use std::sync::Arc;
use aws_lambda_events::event::sqs::SqsEvent;
use lambda_runtime::{service_fn, LambdaEvent};
use serverust_events::router::EventRouter;
use serverust_events::sqs::consumer::SqsBroker;

let broker = Arc::new(SqsBroker::new());
handle_sqs::register(EventRouter::new())
    .attach(broker.clone())
    .await?;

lambda_runtime::run(service_fn(move |event: LambdaEvent<SqsEvent>| {
    let broker = broker.clone();
    async move {
        Ok::<_, lambda_runtime::Error>(broker.handle_sqs_event(&event.payload).await)
    }
}))
.await?;
```

Operacionalmente:

- Habilite `ReportBatchItemFailures` no Event Source Mapping da Lambda.
- Se o handler retorna erro, o `message_id` entra em `batch_item_failures` e a mensagem volta pelo visibility timeout.
- Mensagens sem handler para a fila do ARN, ou sem `event_source_arn` válido, também entram em `batch_item_failures` quando têm `message_id`; isso evita ack silencioso pela Lambda.
- Se a mensagem não tem `message_id`, a falha não pode ser reportada individualmente. O broker registra warning e a Lambda tende a retentar o batch inteiro.

## SQS standalone worker

Use `StandaloneSqsBroker` quando a aplicação roda fora da Lambda e precisa fazer long-poll em SQS:

```rust
use std::sync::Arc;
use serverust_events::router::EventRouter;
use serverust_events::sqs::standalone::{StandaloneConfig, StandaloneSqsBroker};

let broker = Arc::new(
    StandaloneSqsBroker::new(
        receive_client,
        delete_client,
        "https://sqs.us-east-1.amazonaws.com/123/orders".to_string(),
        "orders".to_string(),
    )
    .with_config(StandaloneConfig::default()),
);

handle_sqs::register(EventRouter::new())
    .attach(broker.clone())
    .await?;

broker.run().await?;
```

Defaults confirmados no código:

- `max_messages = 10`
- `wait_time_seconds = 20`
- `error_backoff = 1s`
- handlers com sucesso são apagados via `DeleteMessageBatch`
- handlers com erro não são apagados e retornam pelo visibility timeout, salvo quando uma camada de DLQ absorve a falha
- `signal_shutdown()` para novas chamadas de receive e drena mensagens em voo antes de retornar

## Retry, idempotência e DLQ em SQS

Para SQS, a macro também aceita retry e DLQ declarativos:

```rust
#[subscriber(
    driver = "sqs",
    queue = "orders",
    retry = exponential(max = 5, base = "100ms"),
    dlq = "orders-dlq"
)]
async fn handle_order(event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}
```

As camadas SQS preservam o contrato `tower::Service<SqsMessage>`:

```text
inbound -> TracingLayer -> IdempotencyLayer -> DlqLayer -> RetryLayer -> handler
```

Notas de operação:

- `RetryLayer` usa tentativas imediatas por default e pode aplicar backoff exponencial com teto total.
- `IdempotencyLayer` usa `message_id` como chave e TTL default de 24h. Sem `message_id`, a camada bypassa dedupe e registra warning.
- `DlqLayer` adiciona o atributo `_serverust_failure_reason` e emite métrica `serverust.sqs.dlq_routed`.
- Para idempotência persistente em produção, use um `IdempotencyStore` de `serverust-telemetry` com feature adequada, como DynamoDB.

## FIFO

Subscribers FIFO precisam declarar `fifo` e receber `SqsFifoMetadata`:

```rust
use serverust_events::sqs::extract::SqsFifoMetadata;

#[subscriber(driver = "sqs", queue = "orders.fifo", fifo)]
async fn handle_fifo(
    event: OrderCreated,
    meta: SqsFifoMetadata,
) -> Result<(), BrokerError> {
    println!("group={}", meta.message_group_id);
    Ok(())
}
```

Guards da macro:

- `SqsFifoMetadata` só é aceito em subscribers `driver = "sqs"` com flag `fifo`.
- A flag `fifo` exige que o handler declare `SqsFifoMetadata`.
- `topic` é exclusivo de Kafka; `queue` é exclusivo de SQS.

Para publicar em FIFO, `SqsFifoProducer` usa type-state: `send()` só existe depois de `message_group_id(...)`.

```rust
use serverust_events::sqs::fifo_producer::SqsFifoProducer;
use serverust_events::sqs::producer::ProducerConfig;

let (producer, task) = SqsFifoProducer::new(client, queue_url, ProducerConfig::default());

let message_id = producer
    .send_builder("{\"id\":\"ord_1\"}")
    .message_group_id("customer-123")
    .deduplication_id("ord_1")
    .send()
    .await?;

drop(producer);
task.await?;
```

## Producers SQS standard

`SqsProducer` faz batching transparente:

- flush ao atingir 10 mensagens ou 200ms de linger
- retry exponencial para falhas parciais de `SendMessageBatch`
- shutdown gracioso ao dropar todos os clones ou chamar `signal_shutdown()`

```rust
use serverust_events::sqs::producer::{ProducerConfig, SqsProducer};

let (producer, task) = SqsProducer::new(client, queue_url, ProducerConfig::default());
let message_id = producer.send("{\"id\":\"ord_1\"}").await?;
producer.signal_shutdown();
task.await?;
```

## AsyncAPI

Com a feature `asyncapi`, os tipos Rust geram AsyncAPI 3.0 em YAML:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serverust_events::asyncapi::AsyncApiBuilder;

#[derive(Serialize, Deserialize, JsonSchema)]
struct OrderCreated {
    id: String,
}

let spec = AsyncApiBuilder::new()
    .title("Orders API")
    .version("1.0.0")
    .add_receive::<OrderCreated>("orders")
    .build()
    .to_yaml()?;
```

Também é possível marcar subscribers:

```rust
#[subscriber(driver = "sqs", queue = "orders", asyncapi)]
async fn handle_order(event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}

let builder = handle_order::register_asyncapi(
    AsyncApiBuilder::new().title("Orders API").version("1.0.0"),
);
```

Para integrar com a CLI, o binário da aplicação precisa detectar a flag interna:

```rust
use serverust_events::asyncapi::{emit_asyncapi_if_requested, AsyncApiBuilder};

let builder = handle_order::register_asyncapi(
    AsyncApiBuilder::new().title("Orders API").version("1.0.0"),
);

if emit_asyncapi_if_requested(builder, std::env::args())? {
    return Ok(());
}
```

Depois gere o contrato sem subir consumers/producers:

```bash
serverust info --asyncapi --out asyncapi.yaml
```

## Operações de fila

O CLI inclui diagnósticos SQS simples:

```bash
serverust queue inspect https://sqs.us-east-1.amazonaws.com/123/orders
serverust queue tail https://sqs.us-east-1.amazonaws.com/123/orders --max 10
```

Comportamento atual:

- `inspect` chama `GetQueueAttributes` e mostra mensagens disponíveis, idade da mensagem mais antiga e redrive policy.
- `tail` chama `ReceiveMessage` com `max` entre 1 e 10, mostra preview do body e não deleta mensagens.
- `tail` pode alterar temporariamente a visibilidade das mensagens recebidas, porque usa a semântica normal de `ReceiveMessage`.

## Testes

Use `InMemoryBroker` para handlers independentes de transporte:

```rust
use std::sync::Arc;
use serverust_events::broker::{in_memory::InMemoryBroker, Broker};

let broker = Arc::new(InMemoryBroker::new());
broker.publish("orders.created", br#"{"id":"ord_1"}"#).await?;

let messages = broker.messages("orders.created");
assert_eq!(messages.len(), 1);
```

Para SQS, prefira testar o contrato do handler com mensagens reais de fixture e validar:

- sucesso: retorna `SqsBatchResponse` sem `batch_item_failures`
- erro do handler: inclui o `message_id` em `batch_item_failures`
- fila sem handler ou ARN inválido: não é tratado como sucesso implícito
- FIFO: falha de compilação quando `fifo`/`SqsFifoMetadata` não combinam

## Exemplo completo

Veja [`examples/kafka-wallet/`](../../examples/kafka-wallet/) para o fluxo **wallet.events -> DynamoDB -> wallet.results** usando `#[subscriber]`, `#[publisher]` e `EventRouter`.
