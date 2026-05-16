//! Extractor tipado para registros Kafka recebidos via AWS Lambda.
//!
//! Suporta MSK provisionado, MSK Serverless e self-managed Kafka —
//! todos usam o mesmo [`aws_lambda_events::event::kafka::KafkaEvent`].

use std::collections::HashMap;

use aws_lambda_events::event::kafka::KafkaEvent;
use base64::Engine;
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Erro durante a decodificação de um registro Kafka.
#[derive(Debug, Error)]
pub enum KafkaPayloadError {
    /// O campo `value` do registro estava ausente (`null`).
    #[error("record at {topic}:{partition}+{offset} has no value")]
    MissingValue {
        topic: String,
        partition: i64,
        offset: i64,
    },
    /// Falha ao decodificar o `value` de Base64.
    #[error("base64 decode error at {topic}:{partition}+{offset}: {source}")]
    Base64Decode {
        topic: String,
        partition: i64,
        offset: i64,
        #[source]
        source: base64::DecodeError,
    },
    /// Falha ao desserializar o JSON decodificado para o tipo `T`.
    #[error("json deserialize error at {topic}:{partition}+{offset}: {source}")]
    JsonDeserialize {
        topic: String,
        partition: i64,
        offset: i64,
        #[source]
        source: serde_json::Error,
    },
}

/// Registro Kafka com payload já decodificado e tipado como `T`.
///
/// Construído via [`KafkaRecord::from_kafka_event`] a partir de um
/// [`KafkaEvent`] recebido pela Lambda.
///
/// # Exemplo
///
/// ```rust
/// use serde::Deserialize;
/// use serverust_events::kafka::KafkaRecord;
///
/// #[derive(Deserialize)]
/// struct Payment { amount: u64 }
///
/// // Em um EventHandler<KafkaEvent>:
/// // let records: Vec<KafkaRecord<Payment>> = KafkaRecord::from_kafka_event(&event)?;
/// ```
#[derive(Debug)]
pub struct KafkaRecord<T> {
    /// Payload desserializado do campo `value` (Base64 → JSON → T).
    pub payload: T,
    /// Headers do registro como mapa de nome → bytes.
    pub headers: HashMap<String, Vec<u8>>,
    /// Partição do registro.
    pub partition: i64,
    /// Offset do registro na partição.
    pub offset: i64,
    /// Nome do tópico.
    pub topic: String,
    /// Chave do registro decodificada de Base64, ou `None` se ausente.
    pub key: Option<Vec<u8>>,
}

impl<T: DeserializeOwned> KafkaRecord<T> {
    /// Decodifica todos os registros de um [`KafkaEvent`] para `Vec<KafkaRecord<T>>`.
    ///
    /// Retorna o primeiro erro encontrado durante decodificação.
    /// Os registros são retornados na ordem em que aparecem no evento (por tópico-partição).
    pub fn from_kafka_event(event: &KafkaEvent) -> Result<Vec<Self>, KafkaPayloadError> {
        let mut out = Vec::new();
        for records in event.records.values() {
            for raw in records {
                let topic = raw.topic.clone().unwrap_or_default();
                let partition = raw.partition;
                let offset = raw.offset;

                let value_b64 =
                    raw.value
                        .as_deref()
                        .ok_or_else(|| KafkaPayloadError::MissingValue {
                            topic: topic.clone(),
                            partition,
                            offset,
                        })?;

                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(value_b64)
                    .map_err(|e| KafkaPayloadError::Base64Decode {
                        topic: topic.clone(),
                        partition,
                        offset,
                        source: e,
                    })?;

                let payload = serde_json::from_slice::<T>(&bytes).map_err(|e| {
                    KafkaPayloadError::JsonDeserialize {
                        topic: topic.clone(),
                        partition,
                        offset,
                        source: e,
                    }
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

                out.push(KafkaRecord {
                    payload,
                    headers,
                    partition,
                    offset,
                    topic,
                    key,
                });
            }
        }
        Ok(out)
    }
}
