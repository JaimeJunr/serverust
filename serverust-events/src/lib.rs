//! Suporte event-driven opt-in para serverust.
//!
//! Fornece extractors tipados para event sources AWS e a abstração
//! [`broker::Broker`] sobre transportes event-driven:
//!
//! - [`kafka::KafkaRecord<T>`] — decodifica Base64 + JSON de registros MSK/self-managed Kafka.
//! - [`broker::Broker`] — trait genérica `publish` / `subscribe`.
//! - [`broker::kafka::KafkaBroker`] (feature `kafka`) — implementação Kafka via `rust-rdkafka`.
//! - [`producer::KafkaProducer`] (feature `kafka-producer`) — producer singleton legado, mantido para v0.1.x.

#[cfg(feature = "asyncapi")]
pub mod asyncapi;
pub mod broker;
pub mod extract;
pub mod kafka;
pub mod retry;
pub mod router;
pub mod runtime;
#[cfg(feature = "sqs")]
pub mod sqs;

#[cfg(feature = "kafka-producer")]
pub mod producer;
