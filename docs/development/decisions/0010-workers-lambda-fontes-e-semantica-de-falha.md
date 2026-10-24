# ADR 0010 — Workers Lambda: fontes de evento tipadas e semântica de falha

- **Status:** Proposed
- **Date:** 2026-10-24
- **Deciders:** maintainers serverust

---

## Contexto e Problema

O formato mais comum de serviço em Lambda não é só uma API HTTP. É uma API HTTP **mais um punhado de workers**: funções separadas, disparadas por fila, por objeto novo num bucket ou por agendamento, que compartilham domínio, configuração e injeção de dependências com a API. Um backoffice típico tem algo como:

| Worker | Gatilho | O que ele precisa do framework |
|---|---|---|
| Processador de jobs | SQS, `batchSize: 1`, DLQ por `maxReceiveCount` | separar erro terminal de erro retentável; backoff entre tentativas; saber em qual tentativa está |
| Ponte de arquivos | S3 `ObjectCreated` com filtro de prefixo e sufixo | evento tipado; falha de um record não esconder os outros |
| Executor de rotinas | EventBridge Scheduler, invocação direta com payload próprio | payload tipado; payload inválido não ser retentado para sempre |

O serverust cobre esse formato só em parte. O levantamento abaixo foi feito contra o `main` em 2026-10-24.

### O que existe

- **SQS via event source mapping**, com falha parcial de lote: `SqsBroker::handle_sqs_event` devolve `batch_item_failures` (`serverust-events/src/sqs/consumer.rs:110`), e a macro aceita `driver = "sqs"` (`serverust-macros/src/lib.rs:1177`).
- **SQS long-poll** fora da Lambda (`serverust-events/src/sqs/standalone.rs:132`).
- **MSK/Kafka** via Lambda (`serverust-events/src/broker/lambda.rs:40`).
- **Invocação direta genérica**: `run_event_lambda<E: Deserialize>` (`serverust-lambda/src/lib.rs:109`) e `App::event<E, H>` (`serverust-core/src/app.rs:310`).

### As lacunas

1. **Todo erro é retentável.** Qualquer `Err` do handler vira item de `batch_item_failures` (`serverust-events/src/sqs/consumer.rs:106-110`). Não há como dizer "esta mensagem nunca vai dar certo, descarte". Erro de configuração do cliente ou payload malformado gira até o `maxReceiveCount` e cai na DLQ, poluindo-a com o que não é incidente.
2. **Backoff custa memória × tempo.** O `RetryLayer` espera com `tokio::time::sleep` dentro da mesma invocação (`serverust-events/src/sqs/layers.rs:62-160`). Em Lambda isso é pagar para dormir — e um backoff de 30 s numa função de 300 s consome um décimo do timeout. O jeito barato é devolver a mensagem à fila com `ChangeMessageVisibility`, que hoje só é usado como heartbeat (`serverust-events/src/sqs/heartbeat.rs:49`).
3. **O handler não sabe em qual tentativa está.** `ApproximateReceiveCount` chega em `SqsMetadata.attributes` (`serverust-events/src/sqs/extract.rs:33`), mas nada o lê. O contador do `RetryLayer` zera a cada invocação.
4. **DLQ pela metade.** O `DlqLayer` existe (`serverust-events/src/sqs/layers.rs:353`), mas não há implementação de produção de `DlqClient` — só o mock de teste (`serverust-events/tests/sqs_dlq.rs:59`) — e nenhum ponto o monta no `SqsBroker`.
5. **S3 e EventBridge não existem como fonte.** Só pelo caminho genérico, sem tipo pronto, sem filtro e sem regra de falha por record. A [ADR 0003](0003-event-driven-crate-separada-serverust-events.md) já citava as duas como fontes futuras; a [roadmap](../../product/roadmap.md) as planeja para a v0.4.
6. **Dois caminhos de DI.** `App::event` recebe o `Container` compartilhado com o HTTP (`serverust-core/src/app.rs:307-309`); o `EventRouter` tem estado próprio (`serverust-events/src/router.rs:235`). Quem escreve worker escolhe entre a ergonomia de um e o DI do outro.
7. **Sem DX de worker.** `serverust generate` não cria worker, o scaffold é só HTTP (`serverust-cli/src/templates.rs:64-93`) e `serverust dev` é `cargo watch` (`serverust-cli/src/commands.rs:17-21`), sem poller de fila nem invocação com evento de exemplo. O ciclo de desenvolvimento de um worker hoje é deploy.

Pela [filosofia](../../product/philosophy.md), as lacunas 1 a 4 são o tipo mais caro de default: **permissivo e silencioso**. Nada falha na hora; o custo aparece semanas depois, como DLQ cheia e conta de Lambda alta.

## Drivers de Decisão

- **Invariantes públicos**: `serverust-core` sem deps de eventos; `hello-world` sem Kafka/DynamoDB; binário < 10 MB e cold start < 50 ms p95 não regridem.
- **Custo em Lambda é memória × tempo** — esperar dentro da invocação é o anti-padrão a eliminar, não a otimizar.
- **Falhe fechado; exceção auditável por presença** — descartar uma mensagem é a decisão perigosa, então é ela que se marca.
- **Um worker é uma função** — a separação por binário já é a prática da plataforma e mantém cada cold start mínimo.
- **Macro exige builder equivalente** ([ADR 0007](0007-event-api-design-macro-builder.md)).

## Opções Consideradas

### Topologia

1. **Uma função Lambda por worker, um binário por função** (escolhida) — o que `run()` já impõe ao recusar handlers de evento (`serverust-lambda/src/lib.rs:139-141`). Esta ADR torna isso explícito em vez de deixar como limitação.
2. Um binário que roteia HTTP e eventos pelo formato do payload — rejeitada: puxa `lambda_http` para o worker e as deps de cada fonte para a API, pesando o cold start de todos, e a detecção por formato é frágil (vários eventos AWS se parecem).

### Classificação de erro

1. **Retentável por default; terminal só por marcação explícita** (escolhida).
2. Terminal por default, retentável por marcação — rejeitada: um erro transitório não marcado (timeout de rede) descartaria a mensagem em silêncio. É a falha por omissão que a filosofia proíbe.
3. Classificação por tipo de erro configurada no broker — rejeitada: a regra fica longe do código que sabe se o erro é permanente.

### Backoff

1. **Devolver à fila com `ChangeMessageVisibility`, calculado a partir de `ApproximateReceiveCount`** (escolhida).
2. Reenfileirar com `DelaySeconds` e contador próprio no payload — rejeitada como default: duplica a mensagem, perde o `maxReceiveCount` nativo e exige permissão de `SendMessage`. Fica possível ao usuário.
3. Manter `sleep` na invocação — rejeitada como default; continua disponível para retry curto (dezenas de ms), onde o custo é desprezível.

## Decisão

Workers passam a ser cidadãos de primeira classe em `serverust-events`. Nomes de API abaixo são provisórios; o que esta ADR fixa é o comportamento.

### 1. Um worker é uma função

Cada worker é um binário com seu próprio `main`, chamando `run_event_lambda` (ou o sucessor tipado). A unificação HTTP + eventos num binário fica explicitamente fora de escopo. A documentação passa a mostrar o layout de workspace com API e workers como membros irmãos, compartilhando um crate de domínio.

### 2. Erro terminal é marcado; o resto retenta

O handler pode devolver um erro **terminal** (ex.: `HandlerError::terminal(motivo)`). Para erro terminal o framework:

- **não** coloca a mensagem em `batch_item_failures` — a Lambda a remove da fila;
- emite log estruturado e métrica EMF com o motivo;
- encaminha para a DLQ configurada, se houver (item 4), carregando o motivo como atributo.

Qualquer outro erro continua retentável, exatamente como hoje. Descartar exige escrever `terminal` — `grep` acha cada ponto em que o serviço aceita perder uma mensagem.

O mesmo vale para fontes sem lote: payload que não desserializa num agendamento é terminal por default, porque nenhuma retentativa vai consertá-lo.

### 3. Backoff fora da invocação

Com feature `sqs-backoff` (traz `aws-sdk-sqs`), uma política de backoff por tentativa — ex.: `[1s, 5s, 30s]` — é aplicada assim: o erro retentável chama `ChangeMessageVisibility` com o atraso da tentativa corrente e reporta a mensagem em `batch_item_failures`. A invocação termina na hora.

A tentativa corrente vem de `ApproximateReceiveCount`, exposto ao handler num extractor tipado (ex.: `Attempt(u32)`). A DLQ nativa da fila (`maxReceiveCount`) continua sendo o limite de tentativas; o framework não reimplementa redrive.

### 4. DLQ com motivo, de verdade

`DlqClient` ganha implementação de produção sobre `aws-sdk-sqs` (mesma feature do item 3) e um ponto de montagem documentado no `SqsBroker`. Sem DLQ configurada, a fila depende só do redrive nativo — o caminho default continua sem SDK.

### 5. Fontes novas: S3 e agendamento

Cada uma atrás de feature própria em `serverust-events`, ativando só a feature correspondente de `aws_lambda_events`:

- **`s3`** — `S3Event` tipado, com filtro declarativo de prefixo/sufixo no handler (defesa em profundidade sobre o filtro da notificação). Como invocação assíncrona não tem falha parcial, o framework processa **todos** os records, agrega os erros e falha a invocação no fim se algum record falhou. Um record com erro não impede os seguintes; a retentativa reprocessa todos, então o guia exige handler idempotente e aponta o `IdempotencyStore`.
- **`schedule`** — payload próprio tipado para EventBridge Scheduler (invocação direta) e `ScheduledEvent` para regra de cron. O caso de schedules criados em runtime pela aplicação, via SDK, é suportado: o worker só conhece o payload, não quem o agendou.

### 6. Um caminho de DI

O `EventRouter` passa a aceitar o mesmo `Container` do `App`, para worker e API serem montados pela mesma composição de providers. O `with_state` atual continua como alternativa.

### 7. DX de worker na CLI

- `serverust generate worker <nome> --trigger sqs|s3|schedule` cria o membro do workspace, o `main` e um evento de exemplo.
- `serverust invoke <worker> --event <arquivo.json>` roda o handler local com um evento de fixture, sem AWS.
- `serverust dev --worker <nome>` faz long-poll numa fila de endpoint configurável (LocalStack ou similar) e despacha para o handler — reaproveitando o `StandaloneSqsBroker`.

### Fases

1. Semântica de falha do SQS: itens 2, 3 e 4. É a de maior impacto e não depende de fonte nova.
2. Fontes `s3` e `schedule`: item 5.
3. DI e CLI: itens 6 e 7.

### Fora de escopo

SNS, Kinesis, DynamoDB Streams e Step Functions ficam para ADR futura; continuam acessíveis pelo caminho genérico. A falta de falha parcial de lote no MSK (`serverust-events/src/broker/lambda.rs:87` aborta no primeiro erro) é lacuna conhecida, a tratar com a mesma regra do item 2 quando a fonte Kafka for revisitada.

## Consequências

### Positivas

- Mensagem impossível de processar deixa de girar até a DLQ; a DLQ volta a significar incidente.
- Backoff deixa de custar tempo de execução: a invocação termina e a fila guarda a espera.
- O ponto em que o serviço aceita perder dado é encontrável por `grep`.
- S3 e agendamento, os dois gatilhos mais comuns depois de SQS, entram tipados e com regra de falha definida.
- Worker deixa de ser testado só em deploy.

### Negativas / Trade-offs

- `aws-sdk-sqs` entra na árvore de quem ativa backoff ou DLQ — feature opt-in, mas peso real no binário do worker. Medir antes de fechar a fase 1.
- Uma variante nova num enum público de erro quebra semver se o enum não for `#[non_exhaustive]` (`BrokerError` não é, `serverust-events/src/broker/contract.rs:20`). A fase 1 decide entre aceitar a quebra na próxima minor (pré-1.0) ou carregar o "terminal" num tipo próprio.
- S3 sem falha parcial significa reprocessar records que já deram certo; a regra fica correta só com handler idempotente, e isso é disciplina, não tipo.
- `ChangeMessageVisibility` exige permissão IAM extra na função; a CLI e o guia precisam documentá-la.
- Mais superfície na CLI para manter.

## Verificação

```bash
# serverust-core continua sem deps de eventos
cargo tree -p serverust-core | grep -E "kafka|rdkafka|event" \
  && echo "REGRESSÃO" || echo "ok"

# hello-world não ganha SDK de SQS nem fontes novas
cargo tree -p hello-world --prefix none | awk '{print $1}' | sort -u \
  | grep -xE "aws-sdk-sqs|aws_lambda_events" && echo "REGRESSÃO" || echo "ok"

# o default de serverust-events não puxa o SDK
cargo tree -p serverust-events --no-default-features --features sqs \
  --prefix none | awk '{print $1}' | grep -x aws-sdk-sqs \
  && echo "REGRESSÃO" || echo "ok"

scripts/benchmark_ci.sh
```

Testes de comportamento esperados na fase 1: erro terminal fora de `batch_item_failures`; erro retentável dentro, com `ChangeMessageVisibility` chamado com o atraso da tentativa lida de `ApproximateReceiveCount`; terminal com DLQ configurada chega à DLQ com o motivo.

## Links e Referências

- [ADR 0003](0003-event-driven-crate-separada-serverust-events.md) — `serverust-events` como casa das fontes de evento; S3 e EventBridge previstos
- [ADR 0007](0007-event-api-design-macro-builder.md) — macro exige builder equivalente
- [ADR 0009](0009-auth-authz-crate-separada-serverust-auth.md) — precedente de "falhe fechado, exceção por presença"
- [`docs/product/philosophy.md`](../../product/philosophy.md) — defaults na era dos agentes
- [`docs/product/roadmap.md`](../../product/roadmap.md) — `--trigger` e manifest de fontes na v0.4
- [AWS: partial batch response para SQS](https://docs.aws.amazon.com/lambda/latest/dg/services-sqs-errorhandling.html)
