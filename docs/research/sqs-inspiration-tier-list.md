# Tier List — Inspirações para a API SQS do serverust-events

Pesquisa consolidada sobre libs/frameworks de SQS (e correlatos: task queues, workflow engines, message buses) para informar o design de um futuro adapter SQS no `serverust-events` (provavelmente v0.3+).

Foco: ergonomia de **consumer/producer**, DX, batch handling, falhas parciais, DI, observabilidade, abstração de transporte.

---

## S-Tier — Inspirações obrigatórias

### Celery (Python)

- O padrão-ouro de task queues. Mesmo focado em Redis/RabbitMQ, define o vocabulário do mundo todo: decorators (`@task`), retries com backoff exponencial, chains/groups/chords, dead letter handling.
- O modelo de "task discovery automática" é ouro puro para inspirar como handlers SQS podem ser registrados.
- **Roubar:** decorators declarativos, retry policies como configuração, visibilidade do estado das tasks.

### AWS Lambda Powertools (Python / TS / Java / .NET)

- A referência oficial. O `BatchProcessor` é uma obra de arte: processa mensagens em batch, gerencia falhas parciais (`ReportBatchItemFailures`) automaticamente, integra idempotência.
- Exemplo:
  ```python
  @processor(record_handler=record_handler, processor=processor)
  def lambda_handler(event, context): ...
  ```
- **Roubar:** partial batch failure handling automático, middleware de idempotência, tracing integrado.
- O módulo de **Idempotency** merece menção separada: resolve at-least-once delivery com um decorator, DynamoDB como store, TTL, in-progress tracking.

### Spring Cloud AWS — `@SqsListener` (Java / Kotlin)

- O que se quer emular em ergonomia: anotação na função, deserialização automática, ack/nack implícito por exception, FIFO, polling concorrente configurável.
- **Roubar:** a sensação de "só anotei e funcionou", conversão automática de payload, pooling de conexões transparente.

### Temporal

- Não é "SQS framework", mas muda completamente a forma de pensar filas. Maior referência arquitetural para workflows distribuídos: durable execution, retry automático, state recovery, orquestração declarativa, time-travel/replay determinístico, activities separadas do workflow.
- **Roubar:**
  - Handlers fortemente tipados (`#[workflow] async fn process_payment(input: PaymentInput) -> Result<PaymentResult>`).
  - Retry policies declarativas (`#[retry(max_attempts = 5, backoff = "exponential")]`).
  - **Separação:** workflow orchestration ↔ queue transport ↔ activity execution. O maior erro de frameworks de fila é misturar transporte com lógica de negócio.

### BullMQ (TypeScript / Node)

- O "FastAPI das queues". DX excelente: producer/worker/retries/backoff/concurrency/delay/repeat com quase zero boilerplate.
- API minimalista: `queue.add("send-email", payload)` + `new Worker("emails", async job => {})`.
- Features integradas: retry, delay, rate limit, cron, DLQ, batching, jobs pai/filho (workflows), eventos globais.
- **Roubar:** tipagem genérica `Queue<PayloadType, ResultType>` (perfeita para Rust com generics), API mínima, sistema robusto de eventos.

### MassTransit (C# / .NET)

- A melhor abstração enterprise sobre brokers. Suporta SQS, RabbitMQ, Azure Service Bus, Kafka.
- **Transport abstraction correta:** `cfg.UsingAmazonSqs(...)` vs `cfg.UsingRabbitMq(...)` — código da aplicação não muda. Sugere `#[queue(driver = "sqs")]` / `#[queue(driver = "kafka")]` com o mesmo consumer.
- **Middleware pipeline** maduro: filters, middleware, observers (retry, metrics, tracing, auth). Encaixa perfeitamente com Tower em Rust.
- **Consumer isolation:** `class PaymentConsumer : IConsumer<PaymentCreated>` — cada consumer simples, isolado, tipado.
- **Topologias automáticas:** cria filas/tópicos no SQS/SNS por debaixo dos panos.

---

## A-Tier — Excelentes referências técnicas

### NestJS SQS ecosystem (`@nestjs/microservices`, `@ssut/nestjs-sqs`)

- `@SqsMessageHandler('queue')` + `@SqsConsumerEventHandler('queue', 'error')` + DI nativo, lifecycle hooks, múltiplas filas por módulo.
- **Roubar:** modelo de "module-level config" + handler-level decorators (encaixa no `Container` DI do serverust), auto-discovery de handlers (proc macros em Rust podem fazer isso muito bem), DI integrada (clients, repositories, telemetry, config).

### Watermill (Go)

- Framework de eventos universal. Brilha na abstração de **Envelopes** (metadados + payload) e middlewares genéricos (retry, throttling, poison pill).
- **Roubar:** o conceito de envelope desacoplado do transport.

### Faktory (Go — by Mike Perham, criador do Sidekiq)

- Arquitetura linguagem-agnóstica via protocolo. Separa worker de broker de forma elegante.
- **Roubar:** estudar o protocolo binário e exposição de métricas.

### Sidekiq (Ruby)

- Define o padrão de **middleware chain** para processamento de jobs. Server middleware vs client middleware é uma divisão conceitual brilhante.
- **Roubar:** arquitetura de middleware bidirecional (cabe direitinho em Rust com Tower).

### Symfony Messenger (PHP)

- Melhor sistema de **roteamento baseado em tipos** (`Message -> Handler`), muito parecido com o que Axum faz com tipos de requisição.
- Transport pluggable, message bus uniforme (SQS/Redis/AMQP).
- **Roubar:** roteamento por tipo, abstração de transport para unificar SQS + Kafka no `serverust-events`.

### bbc/sqs-consumer (Node.js)

- API minimalista: `Consumer.create({ queueUrl, handleMessage })`. Batch, visibility extension automática, graceful shutdown.
- **Roubar:** **heartbeat/visibility extension** durante processamento longo — pouca gente faz isso bem.

### Inngest

- Developer experience moderna serverless: event-driven functions, retries automáticos, step execution, durable workflows, observabilidade visual.
- Insight principal: transformar jobs em "functions" (`serve({ id: "process-order", trigger: event("order/created") })`). Combina com o mental model do Lambda.

### Benthos (Go)

- Engine de streaming de dados declarativa. Configuração via YAML com tratamento de erros nativo, retentativas automáticas, mutações de dados integradas. Ápice do "low-code" para filas.

### AWS SDK v3 — Middleware Architecture

- Arquitetura baseada em middlewares: intercepta a mensagem em qualquer etapa (antes de enviar, após receber, antes de serializar) para logs, tracing, modificar payload.

### softwaremill/elasticmq + fs2-aws (Scala)

- Streaming functional: SQS como `Stream[F, Message]`. Backpressure nativo via fs2.
- **Roubar:** modelo de stream com backpressure se expor API async-friendly em Rust (`Stream<Item = SqsMessage>`).

### Shoryuken (Ruby)

- Inspirado no Sidekiq, worker-per-queue com concorrência configurável.
- **Roubar:** modelo de concorrência por queue (workers paralelos por fila).

---

## B-Tier — Referências pontuais

### `aws-lambda-rust-runtime` + `aws_lambda_events::sqs`

- Baseline. Tipos de evento (`SqsEvent`, `SqsBatchResponse`) já existem.
- **Roubar:** reaproveitar `aws_lambda_events::sqs` em vez de reinventar tipos.

### goaws/sqs-consumer-go

- Simples, channel-based, idiomático Go. Referência de minimalismo.

---

## D-Tier — Evitar como modelo

- **boto3 puro / aws-sdk-rust direto no handler** — sem ergonomia, força usuário a reescrever batch/partial-failure toda vez.
- **Wrappers "thin" sem partial batch response** — qualquer lib que não trate `batchItemFailures` é armadilha em produção.

---

## Padrões a canibalizar no serverust-events (síntese)

Posicionamento: Axum + FastAPI + NestJS + Temporal + MassTransit + BullMQ.

### 1. Extractors estilo Axum/FastAPI para mensagens SQS

```rust
async fn process_user_signup(
    Json(payload): Json<NewUserPayload>,
    meta: SqsMetadata,
    State(repo): State<Arc<UserRepo>>,
) -> Result<(), MyError> {
    repo.create(payload).await
}
```

### 2. Partial batch failure como first-class citizen

- Powertools faz isso bem, mas com sintaxe Java/Python. Em Rust fica ainda mais idiomático: handler retorna `Result<(), Error>` por mensagem; framework agrega `batchItemFailures` automaticamente. Impossível esquecer de reportar falha individual (type-safe).

### 3. Middleware pipeline com Tower

- Maior diferencial possível:

```rust
ServiceBuilder::new()
    .layer(RetryLayer)
    .layer(TracingLayer)
    .layer(MetricsLayer)
    .layer(IdempotencyLayer)
```

### 4. Idempotency como middleware opt-in

- Powertools mostrou o caminho. Reaproveitar `IdempotencyStore` de `serverust-telemetry`.

### 5. FIFO vs Standard como tipo no compilador

- `SqsHandler<Fifo>` vs `SqsHandler<Standard>` força configurações corretas em compile-time. Rust permite de um jeito que outras linguagens não.

### 6. Visibility timeout extension automática

- Heartbeat para handlers longos — bbc/sqs-consumer é referência.

### 7. DLQ routing declarativo

- Anotar o handler com política de retry e destino do DLQ.

### 8. Auto-Delete inteligente

- Delete automático no sucesso; libera para visibility timeout no erro. Usuário não escreve esse código.

### 9. Backoff exponencial no polling

- Long polling com diminuição de ritmo quando fila vazia para evitar custo em API calls vazias.

### 10. Batching transparente

- Aceitar lista de mensagens e agrupar em lotes de até 10 automaticamente.

### 11. Transport abstraction (MassTransit)

```rust
#[queue(driver = "sqs", name = "orders")]
#[queue(driver = "kafka", topic = "orders")]
#[queue(driver = "rabbitmq", queue = "orders")]
async fn handle_order(msg: OrderCreated) -> Result<()> { ... }
```

### 12. Workflow primitives (Temporal)

```rust
workflow!(
    validate_order
        -> reserve_stock
        -> process_payment
        -> ship_order
)
```

---

## Recomendação de mergulho

Se for ler código de três projetos antes de escrever o adapter SQS:

1. **aws-lambda-powertools-python** — módulos `batch` e `idempotency` para o handling de SQS especificamente.
2. **Axum** — reler com olhos de "como aplicar isso em event-driven".
3. **NestJS `@nestjs/microservices` (transport SQS)** — design de decorators + DI em framework opinionated.

Bônus: **MassTransit** para arquitetura de transport abstraction e **BullMQ** para DX mínima.

---

## Tier list final consolidada

| Tier | Projeto | Principal contribuição para o design |
|------|---------|---------------------------------------|
| S | Celery | DX / decorators / retry policies |
| S | AWS Lambda Powertools | Partial batch failure + idempotency |
| S | Spring Cloud AWS (`@SqsListener`) | "Anotei e funcionou" |
| S | Temporal | Durable workflows / separação de camadas |
| S | BullMQ | Melhor DX / tipagem genérica |
| S | MassTransit | Transport abstraction / middleware pipeline |
| A | NestJS SQS ecosystem | Decorators + DI + auto-discovery |
| A | Watermill | Envelope abstraction |
| A | Faktory | Protocolo agnóstico de linguagem |
| A | Sidekiq | Middleware chain bidirecional |
| A | Symfony Messenger | Roteamento por tipo |
| A | bbc/sqs-consumer | Visibility extension |
| A | Inngest | Jobs como funções / DX serverless |
| A | Benthos | Streaming declarativo |
| A | AWS SDK v3 middleware | Interceptação por etapa |
| A | fs2-aws (Scala) | Backpressure como stream |
| A | Shoryuken | Concorrência por queue |
| B | aws-lambda-rust-runtime | Tipos base reaproveitáveis |
| B | goaws/sqs-consumer-go | Minimalismo channel-based |
| D | SDK puro / wrappers thin | Sem ergonomia, sem partial failure |
