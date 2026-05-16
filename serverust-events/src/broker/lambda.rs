//! Broker para o modo AWS Lambda (US-7).
//!
//! Em Lambda, o transporte Kafka é resolvido pelo runtime AWS: registros
//! chegam empacotados em [`aws_lambda_events::event::kafka::KafkaEvent`]
//! como invocação da função. Não há poll loop nem conexão com o broker
//! físico — por isso este módulo é independente da feature `kafka` e não
//! puxa `rdkafka`.
//!
//! Uso típico (junto com [`crate::runtime::Runtime`]):
//!
//! ```ignore
//! let broker = Arc::new(LambdaBroker::new());
//! router.attach(broker.clone()).await?;
//! lambda_runtime::run(service_fn(|event: LambdaEvent<KafkaEvent>| {
//!     let broker = broker.clone();
//!     async move { broker.handle_kafka_event(&event.payload).await }
//! })).await?;
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use aws_lambda_events::event::kafka::KafkaEvent;
use base64::Engine;

use super::{BoxedHandler, Broker, BrokerError, BrokerMessage};

/// Broker sink-only para o modo Lambda.
///
/// - `subscribe` registra handlers em memória.
/// - `handle_kafka_event(&KafkaEvent)` decodifica registros e despacha
///   para os handlers inscritos por tópico (chave do despacho é
///   `record.topic`, não a chave do mapa `records` do payload).
/// - `publish` falha com erro indicando que o broker é sink-only:
///   publicar pertence ao [`crate::broker::kafka::KafkaBroker`] (feature `kafka`)
///   ou ao producer dedicado em uma futura US.
pub struct LambdaBroker {
    subscriptions: Mutex<Vec<Subscription>>,
}

struct Subscription {
    topic: String,
    handler: BoxedHandler,
}

impl LambdaBroker {
    /// Cria um broker Lambda vazio.
    pub fn new() -> Self {
        Self {
            subscriptions: Mutex::new(Vec::new()),
        }
    }

    /// Lista os tópicos atualmente inscritos (ordem de inscrição).
    pub fn subscribed_topics(&self) -> Vec<String> {
        self.subscriptions
            .lock()
            .expect("subscriptions mutex poisoned")
            .iter()
            .map(|s| s.topic.clone())
            .collect()
    }

    /// Despacha todos os registros de `event` para os handlers inscritos.
    ///
    /// Para cada `KafkaRecord` do payload:
    /// 1. Identifica o tópico via `record.topic` (campo do próprio registro).
    /// 2. Se houver handlers inscritos, decodifica `value` (Base64) e
    ///    despacha como [`BrokerMessage`].
    /// 3. Se não houver handlers inscritos para o tópico, ignora o registro.
    ///
    /// O primeiro erro encontrado interrompe o despacho e propaga.
    pub async fn handle_kafka_event(&self, event: &KafkaEvent) -> Result<(), BrokerError> {
        for records in event.records.values() {
            for raw in records {
                let topic = raw.topic.clone().unwrap_or_default();

                let handlers: Vec<BoxedHandler> = self
                    .subscriptions
                    .lock()
                    .map_err(|_| BrokerError::Subscribe("subscriptions mutex poisoned".into()))?
                    .iter()
                    .filter(|s| s.topic == topic)
                    .map(|s| s.handler.clone())
                    .collect();

                if handlers.is_empty() {
                    continue;
                }

                let value_b64 = raw.value.as_deref().ok_or_else(|| {
                    BrokerError::Subscribe(format!(
                        "kafka record {}:{}+{} has no value",
                        topic, raw.partition, raw.offset
                    ))
                })?;

                let payload = base64::engine::general_purpose::STANDARD
                    .decode(value_b64)
                    .map_err(|e| {
                        BrokerError::Subscribe(format!(
                            "base64 decode error at {}:{}+{}: {e}",
                            topic, raw.partition, raw.offset
                        ))
                    })?;

                let key = raw
                    .key
                    .as_deref()
                    .and_then(|k| base64::engine::general_purpose::STANDARD.decode(k).ok());

                let headers: HashMap<String, Vec<u8>> = raw
                    .headers
                    .iter()
                    .flat_map(|h| {
                        h.iter()
                            .map(|(k, v)| (k.clone(), v.iter().map(|b| *b as u8).collect()))
                    })
                    .collect();

                let timestamp = Some(raw.timestamp.timestamp_millis());

                let msg = BrokerMessage {
                    topic: topic.clone(),
                    partition: Some(raw.partition),
                    offset: Some(raw.offset),
                    key,
                    payload,
                    headers,
                    timestamp,
                };

                for handler in handlers {
                    handler(msg.clone()).await?;
                }
            }
        }
        Ok(())
    }
}

impl Default for LambdaBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Broker for LambdaBroker {
    async fn subscribe(&self, topic: &str, handler: BoxedHandler) -> Result<(), BrokerError> {
        self.subscriptions
            .lock()
            .map_err(|_| BrokerError::Subscribe("subscriptions mutex poisoned".into()))?
            .push(Subscription {
                topic: topic.to_string(),
                handler,
            });
        Ok(())
    }

    async fn publish(&self, _topic: &str, _payload: &[u8]) -> Result<(), BrokerError> {
        Err(BrokerError::Publish(
            "LambdaBroker is sink-only: use KafkaBroker (feature `kafka`) to publish".to_string(),
        ))
    }
}
