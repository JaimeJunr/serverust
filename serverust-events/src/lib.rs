//! Suporte event-driven opt-in para serverust.
//!
//! Fornece extractors tipados para event sources AWS:
//! - [`kafka::KafkaRecord<T>`] — decodifica Base64 + JSON de registros MSK/self-managed Kafka.
//! - [`producer::KafkaProducer`] (feature `kafka-producer`) — publica em
//!   tópicos Kafka/MSK com IAM SASL opcional.

pub mod kafka;

#[cfg(feature = "kafka-producer")]
pub mod producer;
