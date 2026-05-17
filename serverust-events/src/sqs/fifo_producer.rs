//! Producer SQS FIFO com builder type-state — guarda de compile-time para
//! `message_group_id` (US-005).
//!
//! Em FIFO o `MessageGroupId` é obrigatório por mensagem. Para evitar erros de
//! configuração só visíveis em runtime, a API expõe um builder cujo método
//! `send()` só existe no estado [`HasGroupId`]. Tentar publicar sem chamar
//! `message_group_id(...)` antes => erro de compilação.
//!
//! Internamente reutiliza o batching, retry e shutdown do [`super::producer::SqsProducer`].
//!
//! # Uso
//!
//! ```rust,ignore
//! let (producer, task) = SqsFifoProducer::new(client, queue_url, ProducerConfig::default());
//! let msg_id = producer
//!     .send_builder("body")
//!     .message_group_id("group-1")
//!     .deduplication_id("dedup-1") // opcional
//!     .send()
//!     .await?;
//! drop(producer);
//! task.await.unwrap();
//! ```

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use tokio::task::JoinHandle;

use super::producer::{MessageId, ProducerConfig, SendClient, SendError, SqsProducer};

/// Producer SQS específico para filas FIFO.
///
/// Clone barato — compartilha o canal e a task de background do
/// [`SqsProducer`] interno. Use [`Self::send_builder`] para publicar mensagens
/// com guarda de compile-time para `message_group_id`.
#[derive(Clone)]
pub struct SqsFifoProducer {
    inner: SqsProducer,
}

impl SqsFifoProducer {
    /// Cria um novo producer FIFO e retorna `(producer, task_handle)`.
    ///
    /// O handle encerra quando todos os clones são dropados ou após
    /// [`SqsProducer::signal_shutdown`].
    pub fn new(
        client: Arc<dyn SendClient>,
        queue_url: impl Into<String>,
        config: ProducerConfig,
    ) -> (Self, JoinHandle<()>) {
        let (inner, task) = SqsProducer::new(client, queue_url, config);
        (Self { inner }, task)
    }

    /// Inicia a construção de uma mensagem FIFO. O builder retornado está no
    /// estado [`NoGroupId`] e ainda **não** pode ser enviado — é obrigatório
    /// chamar [`FifoSendBuilder::message_group_id`] antes de [`FifoSendBuilder::send`].
    pub fn send_builder(&self, body: impl Into<String>) -> FifoSendBuilder<'_, NoGroupId> {
        FifoSendBuilder {
            producer: &self.inner,
            body: body.into(),
            group_id: None,
            dedup_id: None,
            attributes: HashMap::new(),
            _state: PhantomData,
        }
    }

    /// Sinaliza shutdown gracioso. Veja [`SqsProducer::signal_shutdown`].
    pub fn signal_shutdown(&self) {
        self.inner.signal_shutdown();
    }
}

/// Estado type-state: `message_group_id` ainda **não** foi fornecido.
pub struct NoGroupId;
/// Estado type-state: `message_group_id` está presente. Habilita [`FifoSendBuilder::send`].
pub struct HasGroupId;

/// Builder de envio para [`SqsFifoProducer`].
///
/// O parâmetro de tipo `S` codifica se o `message_group_id` já foi fornecido.
/// O método [`Self::send`] só está disponível no estado [`HasGroupId`], então o
/// compilador rejeita chamadas que esqueçam de informar o grupo.
pub struct FifoSendBuilder<'a, S> {
    producer: &'a SqsProducer,
    body: String,
    group_id: Option<String>,
    dedup_id: Option<String>,
    attributes: HashMap<String, String>,
    _state: PhantomData<S>,
}

impl<'a> FifoSendBuilder<'a, NoGroupId> {
    /// Define o `message_group_id` da mensagem e avança o builder para o estado
    /// [`HasGroupId`], onde [`Self::send`] passa a estar disponível.
    pub fn message_group_id(self, id: impl Into<String>) -> FifoSendBuilder<'a, HasGroupId> {
        FifoSendBuilder {
            producer: self.producer,
            body: self.body,
            group_id: Some(id.into()),
            dedup_id: self.dedup_id,
            attributes: self.attributes,
            _state: PhantomData,
        }
    }
}

impl<'a, S> FifoSendBuilder<'a, S> {
    /// Define o `message_deduplication_id` (opcional — use quando a fila não
    /// tem content-based dedupe habilitado).
    pub fn deduplication_id(mut self, id: impl Into<String>) -> Self {
        self.dedup_id = Some(id.into());
        self
    }

    /// Adiciona um atributo customizado à mensagem.
    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

impl<'a> FifoSendBuilder<'a, HasGroupId> {
    /// Enfileira a mensagem no batch interno e aguarda a confirmação do SQS.
    ///
    /// Disponível apenas no estado [`HasGroupId`], garantindo em compile-time
    /// que `message_group_id` foi fornecido.
    pub async fn send(self) -> Result<MessageId, SendError> {
        let group_id = self
            .group_id
            .expect("HasGroupId implica message_group_id presente");
        self.producer
            .enqueue(self.body, self.attributes, Some(group_id), self.dedup_id)
            .await
    }
}
