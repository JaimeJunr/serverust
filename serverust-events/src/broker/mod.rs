//! Trait `Broker` e tipos públicos para abstração de transporte event-driven.
//!
//! A trait `Broker` representa o contrato mínimo de um broker capaz de:
//!
//! - publicar mensagens em um tópico (`publish`);
//! - registrar handlers para receber mensagens de um tópico (`subscribe`).
//!
//! Implementações concretas vivem em submódulos atrás de feature flags:
//!
//! - [`kafka::KafkaBroker`] (feature `kafka`) — usa `rust-rdkafka`.
//! - [`in_memory::InMemoryBroker`] (feature `in-memory`) — entrega em memória, sem infraestrutura.
//!
//! A trait é deliberadamente independente de Kafka — `serverust-core` não
//! precisa importar este módulo nem suas implementações concretas.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

#[cfg(feature = "in-memory")]
pub mod in_memory;

#[cfg(feature = "kafka")]
pub mod kafka;

/// Erros retornados por implementações de [`Broker`].
#[derive(Debug, Error)]
pub enum BrokerError {
    /// Configuração ausente ou inválida no momento da construção do broker.
    #[error("broker configuration error: {0}")]
    Configuration(String),

    /// Falha durante publicação.
    #[error("broker publish error: {0}")]
    Publish(String),

    /// Falha durante inscrição.
    #[error("broker subscribe error: {0}")]
    Subscribe(String),

    /// Erro de transporte/conexão com o broker subjacente.
    #[error("broker transport error: {0}")]
    Transport(String),
}

/// Mensagem entregue a um handler inscrito.
///
/// Representa um registro de broker já normalizado para um shape comum
/// independente do transporte (Kafka, in-memory, etc.).
#[derive(Debug, Clone)]
pub struct BrokerMessage {
    /// Tópico/destino de origem.
    pub topic: String,
    /// Partição (Kafka) ou `None` para brokers sem particionamento.
    pub partition: Option<i64>,
    /// Offset na partição, quando aplicável.
    pub offset: Option<i64>,
    /// Chave do registro, quando presente.
    pub key: Option<Vec<u8>>,
    /// Payload bruto (deserialização fica a cargo do handler/extractors).
    pub payload: Vec<u8>,
    /// Headers como mapa nome → bytes.
    pub headers: HashMap<String, Vec<u8>>,
    /// Timestamp do registro em milissegundos (epoch), quando disponível.
    pub timestamp: Option<i64>,
}

/// Future retornado por um handler inscrito.
pub type HandlerFuture = Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>>;

/// Handler boxado: recebe `BrokerMessage` e devolve uma `HandlerFuture`.
///
/// Compartilhável (`Arc`) para permitir registro do mesmo handler em
/// múltiplas inscrições sem clonagem custosa.
pub type BoxedHandler = Arc<dyn Fn(BrokerMessage) -> HandlerFuture + Send + Sync>;

/// Contrato de broker event-driven.
///
/// Implementações devem ser `Send + Sync` para uso em runtimes async
/// multi-thread e via trait objects.
#[async_trait]
pub trait Broker: Send + Sync {
    /// Registra um handler para receber mensagens publicadas em `topic`.
    ///
    /// O método é não-bloqueante: a ativação real do consumo pode ocorrer
    /// preguiçosamente (drive externo em runtimes Lambda) ou imediatamente
    /// (consumer loop em runtimes long-running).
    async fn subscribe(&self, topic: &str, handler: BoxedHandler) -> Result<(), BrokerError>;

    /// Publica `payload` no `topic`.
    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), BrokerError>;
}

/// Permite usar `Arc<B>` diretamente como `Broker`, facilitando compartilhamento
/// entre handlers e o loop de retry/DLQ.
#[async_trait]
impl<B: Broker> Broker for Arc<B> {
    async fn subscribe(&self, topic: &str, handler: BoxedHandler) -> Result<(), BrokerError> {
        (**self).subscribe(topic, handler).await
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), BrokerError> {
        (**self).publish(topic, payload).await
    }
}
