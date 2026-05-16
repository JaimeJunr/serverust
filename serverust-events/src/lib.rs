//! Suporte event-driven opt-in para serverust.
//!
//! Fornece extractors tipados para event sources AWS:
//! - [`kafka::KafkaRecord<T>`] — decodifica Base64 + JSON de registros MSK/self-managed Kafka.

pub mod kafka;
