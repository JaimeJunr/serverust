//! `SqsBroker` — broker sink-only para SQS no modo AWS Lambda (event source mapping).
//!
//! Em Lambda ESM, o transporte SQS é resolvido pelo runtime AWS: mensagens
//! chegam empacotadas em [`aws_lambda_events::event::sqs::SqsEvent`] como
//! invocação da função. Não há poll loop nem chamadas a `ReceiveMessage` no
//! broker — apenas dispatch dos registros já recebidos.
//!
//! Uso típico:
//!
//! ```ignore
//! use std::sync::Arc;
//! use aws_lambda_events::event::sqs::SqsEvent;
//! use lambda_runtime::{service_fn, LambdaEvent};
//! use serverust_events::router::EventRouter;
//! use serverust_events::sqs::consumer::SqsBroker;
//!
//! # async fn handle(_: ()) -> Result<(), serverust_events::broker::BrokerError> { Ok(()) }
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let broker = Arc::new(SqsBroker::new());
//! EventRouter::new()
//!     .subscribe::<(), _, _>("orders", handle)
//!     .attach(broker.clone())
//!     .await?;
//!
//! lambda_runtime::run(service_fn(move |event: LambdaEvent<SqsEvent>| {
//!     let broker = broker.clone();
//!     async move { Ok::<_, lambda_runtime::Error>(broker.handle_sqs_event(&event.payload).await) }
//! })).await?;
//! # Ok(()) }
//! ```
//!
//! Routing: o nome da fila é extraído do segmento final do
//! `event_source_arn` (`arn:aws:sqs:<region>:<account>:<queue-name>`) e usado
//! como chave de despacho — equivalente ao `topic` da [`crate::broker::Broker`]
//! trait.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use aws_lambda_events::event::sqs::{SqsBatchResponse, SqsEvent, SqsMessage};

use crate::broker::{BoxedHandler, Broker, BrokerError, BrokerMessage};

/// Nome do header em [`BrokerMessage::headers`] usado para transportar a
/// SqsMessage original (JSON-encoded) até o extractor [`super::extract::SqsMetadata`].
///
/// É um detalhe de implementação: usuários do framework não devem ler nem
/// escrever esse header diretamente.
pub(crate) const SQS_METADATA_HEADER: &str = "__serverust_sqs_message";

/// Broker sink-only para o modo Lambda ESM com SQS.
///
/// - [`Self::subscribe`] registra handlers em memória por nome de fila.
/// - [`Self::handle_sqs_event`] despacha cada [`SqsMessage`] do batch para os
///   handlers inscritos e devolve uma [`SqsBatchResponse`] com `batch_item_failures`
///   contendo o `message_id` de cada mensagem cujo handler retornou `Err`.
/// - [`Self::publish`] erra com mensagem clara: este broker é sink-only — para
///   publicar use o `SqsProducer` (US-004) ou outro driver dedicado.
pub struct SqsBroker {
    subscriptions: Mutex<Vec<Subscription>>,
}

struct Subscription {
    queue: String,
    handler: BoxedHandler,
}

impl SqsBroker {
    /// Cria um broker SQS vazio.
    pub fn new() -> Self {
        Self {
            subscriptions: Mutex::new(Vec::new()),
        }
    }

    /// Lista as filas inscritas na ordem de registro.
    pub fn subscribed_queues(&self) -> Vec<String> {
        self.subscriptions
            .lock()
            .expect("sqs subscriptions mutex poisoned")
            .iter()
            .map(|s| s.queue.clone())
            .collect()
    }

    /// Despacha cada mensagem de `event` para os handlers inscritos na fila
    /// correspondente (derivada do `event_source_arn` da mensagem).
    ///
    /// Comportamento:
    ///
    /// 1. Para cada [`SqsMessage`], identifica a fila via segmento final do
    ///    ARN. Mensagens sem ARN são ignoradas (configuração inválida).
    /// 2. Se houver handler inscrito, monta um [`BrokerMessage`] com o body
    ///    em `payload` e a SqsMessage original serializada em
    ///    `headers[SQS_METADATA_HEADER]`, depois invoca o handler.
    /// 3. Em caso de `Err` do handler, registra o `message_id` em
    ///    `batch_item_failures` (modelo de partial batch failure do Lambda ESM).
    /// 4. Mensagens sem `message_id` em erro não podem ser reportadas — o erro
    ///    é logado em stderr; nesse cenário Lambda retentará o batch inteiro.
    /// 5. Mensagens sem handler inscrito são ignoradas (sem falha — em
    ///    Lambda ESM uma fila sem subscriber é misconfiguração).
    pub async fn handle_sqs_event(&self, event: &SqsEvent) -> SqsBatchResponse {
        let mut response = SqsBatchResponse::default();

        for raw in &event.records {
            let Some(queue) = extract_queue_name(raw) else {
                continue;
            };

            let handlers: Vec<BoxedHandler> = self
                .subscriptions
                .lock()
                .expect("sqs subscriptions mutex poisoned")
                .iter()
                .filter(|s| s.queue == queue)
                .map(|s| s.handler.clone())
                .collect();

            if handlers.is_empty() {
                continue;
            }

            let msg = build_broker_message(&queue, raw);

            let mut handler_err: Option<BrokerError> = None;
            for handler in handlers {
                if let Err(e) = handler(msg.clone()).await {
                    handler_err = Some(e);
                    break;
                }
            }

            if let Some(e) = handler_err {
                if let Some(id) = raw.message_id.clone() {
                    response.add_failure(id);
                } else {
                    eprintln!(
                        "[serverust-events] sqs message in queue {queue} failed but has no message_id; \
                         Lambda will retry the whole batch. Underlying error: {e}"
                    );
                }
            }
        }

        response
    }
}

impl Default for SqsBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Broker for SqsBroker {
    async fn subscribe(&self, topic: &str, handler: BoxedHandler) -> Result<(), BrokerError> {
        self.subscriptions
            .lock()
            .expect("sqs subscriptions mutex poisoned")
            .push(Subscription {
                queue: topic.to_string(),
                handler,
            });
        Ok(())
    }

    async fn publish(&self, _topic: &str, _payload: &[u8]) -> Result<(), BrokerError> {
        Err(BrokerError::Publish(
            "SqsBroker is sink-only for Lambda ESM: use SqsProducer (US-004) to send messages"
                .to_string(),
        ))
    }
}

/// Extrai o nome da fila do `event_source_arn` (segmento final).
///
/// Aceita ARNs no formato `arn:aws:sqs:<region>:<account>:<queue-name>` —
/// retorna `Some("queue-name")`. Retorna `None` se o campo está ausente ou
/// não tem o formato esperado.
fn extract_queue_name(msg: &SqsMessage) -> Option<String> {
    let arn = msg.event_source_arn.as_deref()?;
    arn.rsplit(':').next().map(|s| s.to_string())
}

/// Constrói um [`BrokerMessage`] a partir de uma [`SqsMessage`].
///
/// - `payload` = bytes do `body` (string `""` se ausente).
/// - `key` = bytes do `message_id` (se presente).
/// - `headers[SQS_METADATA_HEADER]` = JSON da `SqsMessage` original — consumido
///   apenas pelo extractor [`super::extract::SqsMetadata`]. `SqsMessage` é
///   `Serialize` por definição (aws_lambda_events); o unwrap é seguro.
/// - `timestamp` = `SentTimestamp` do `attributes` quando parseável como `i64`.
pub(crate) fn build_broker_message(queue: &str, msg: &SqsMessage) -> BrokerMessage {
    let payload = msg
        .body
        .as_deref()
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();

    let key = msg.message_id.as_deref().map(|id| id.as_bytes().to_vec());

    let timestamp = msg
        .attributes
        .get("SentTimestamp")
        .and_then(|v| v.parse::<i64>().ok());

    let metadata_json = serde_json::to_vec(msg).expect("SqsMessage is always serializable");

    let mut headers: HashMap<String, Vec<u8>> = HashMap::new();
    headers.insert(SQS_METADATA_HEADER.to_string(), metadata_json);

    BrokerMessage {
        topic: queue.to_string(),
        partition: None,
        offset: None,
        key,
        payload,
        headers,
        timestamp,
    }
}
