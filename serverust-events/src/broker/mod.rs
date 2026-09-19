//! Trait `Broker` e tipos públicos para abstração de transporte event-driven.
//!
//! Implementações concretas vivem em submódulos atrás de feature flags:
//!
//! - [`kafka::KafkaBroker`] (feature `kafka`) — usa `rust-rdkafka`.
//! - [`in_memory::InMemoryBroker`] (feature `in-memory`) — entrega em memória, sem infraestrutura.

mod contract;
pub use contract::*;

#[cfg(feature = "in-memory")]
pub mod in_memory;

#[cfg(feature = "kafka")]
pub mod kafka;

pub mod lambda;
