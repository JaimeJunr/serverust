//! `StandaloneSqsBroker` — broker long-poll para SQS em workers ECS/EC2 (US-010).
//!
//! Em modo Lambda ESM, o transporte SQS é resolvido pelo runtime AWS: mensagens
//! chegam empacotadas em um `SqsEvent` como invocação. Em ECS/EC2 não há esse
//! runtime — o worker precisa chamar `ReceiveMessage` em loop. Este módulo
//! provê essa camada:
//!
//! - [`ReceiveClient`] — trait que abstrai `ReceiveMessage` (mockável; integra
//!   com `aws-sdk-sqs` em produção).
//! - [`StandaloneSqsBroker`] — implementa [`crate::broker::Broker`]: a mesma
//!   macro `#[subscriber]` que funciona com [`super::consumer::SqsBroker`]
//!   (Lambda ESM) funciona aqui sem mudar código de negócio.
//! - [`StandaloneSqsBroker::run`] — long-poll loop com `WaitTimeSeconds=20`
//!   (default), dispatch concorrente das mensagens recebidas e
//!   `DeleteMessageBatch` para as mensagens processadas com sucesso.
//! - [`StandaloneSqsBroker::signal_shutdown`] — graceful shutdown: drena
//!   mensagens em voo antes de [`StandaloneSqsBroker::run`] retornar.
//!
//! # Uso típico
//!
//! ```ignore
//! use std::sync::Arc;
//! use serverust_events::router::EventRouter;
//! use serverust_events::sqs::standalone::{StandaloneSqsBroker, StandaloneConfig};
//!
//! let receive = Arc::new(MySqsReceiveClient::from_aws_sdk());
//! let delete = Arc::new(MySqsDeleteClient::from_aws_sdk());
//!
//! let broker = Arc::new(StandaloneSqsBroker::new(
//!     receive,
//!     delete,
//!     "https://sqs.us-east-1.amazonaws.com/123/orders".into(),
//!     "orders".into(),
//! ));
//!
//! EventRouter::new()
//!     .subscribe::<Order, _, _>("orders", handle_order)
//!     .attach(broker.clone())
//!     .await?;
//!
//! // SIGTERM-handler que dispara graceful shutdown
//! let broker_for_signal = broker.clone();
//! tokio::spawn(async move {
//!     tokio::signal::ctrl_c().await.ok();
//!     broker_for_signal.signal_shutdown();
//! });
//!
//! broker.run().await?;
//! ```

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use aws_lambda_events::event::sqs::SqsMessage;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::broker::{BoxedHandler, Broker, BrokerError};
use crate::sqs::consumer::build_broker_message;
use crate::sqs::delete::{DeleteClient, DeleteEntry, DeleteManager};

// --------------------------------------------------------------------------
// ReceiveClient trait
// --------------------------------------------------------------------------

/// Resultado de [`ReceiveClient::receive`].
#[derive(Debug, Clone, Default)]
pub struct ReceiveResult {
    /// Mensagens recebidas (vazio em long-poll sem mensagens disponíveis).
    pub messages: Vec<SqsMessage>,
}

/// Trait que abstrai a chamada AWS `ReceiveMessage`.
///
/// Implemente sobre `aws-sdk-sqs` em produção; use mocks em testes.
#[async_trait]
pub trait ReceiveClient: Send + Sync + 'static {
    /// Recebe até `max_messages` mensagens com `wait_time_seconds` de long-poll.
    async fn receive(
        &self,
        queue_url: &str,
        max_messages: i32,
        wait_time_seconds: i32,
    ) -> Result<ReceiveResult, String>;
}

// --------------------------------------------------------------------------
// Config
// --------------------------------------------------------------------------

/// Configuração do [`StandaloneSqsBroker`].
#[derive(Debug, Clone)]
pub struct StandaloneConfig {
    /// `MaxNumberOfMessages` por chamada de receive (1..=10). Default 10.
    pub max_messages: i32,
    /// `WaitTimeSeconds` (long-poll). Default 20.
    pub wait_time_seconds: i32,
    /// Backoff entre chamadas que falharam (rede/throttle). Default 1s.
    pub error_backoff: Duration,
}

impl Default for StandaloneConfig {
    fn default() -> Self {
        Self {
            max_messages: 10,
            wait_time_seconds: 20,
            error_backoff: Duration::from_secs(1),
        }
    }
}

// --------------------------------------------------------------------------
// StandaloneSqsBroker
// --------------------------------------------------------------------------

struct Subscription {
    queue: String,
    handler: BoxedHandler,
}

/// Broker long-poll para SQS em workers ECS/EC2.
///
/// Compartilha o contrato [`Broker`] com [`super::consumer::SqsBroker`]: a
/// mesma macro `#[subscriber]` (e o mesmo [`crate::router::EventRouter`])
/// funciona em ambos os modos sem mudar o código do handler.
///
/// O broker é vinculado a UMA fila (par `queue_url`/`queue_name`); para
/// consumir múltiplas filas instancie um broker por fila, cada um com sua
/// task `run()` rodando em paralelo.
pub struct StandaloneSqsBroker<R, D> {
    receive: Arc<R>,
    delete_manager: Arc<DeleteManager<D>>,
    queue_url: String,
    queue_name: String,
    config: StandaloneConfig,
    subscriptions: Arc<Mutex<Vec<Subscription>>>,
    shutdown_tx: watch::Sender<bool>,
}

impl<R, D> StandaloneSqsBroker<R, D>
where
    R: ReceiveClient,
    D: DeleteClient + 'static,
{
    /// Cria um broker para a fila identificada por `queue_url`. O `queue_name`
    /// é usado como chave de dispatch (equivalente ao `topic` da trait
    /// [`Broker`]) — handlers inscritos via [`Broker::subscribe`] com esse
    /// nome recebem as mensagens dessa fila.
    pub fn new(receive: Arc<R>, delete: Arc<D>, queue_url: String, queue_name: String) -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            receive,
            delete_manager: Arc::new(DeleteManager::new(delete).with_zero_backoff()),
            queue_url,
            queue_name,
            config: StandaloneConfig::default(),
            subscriptions: Arc::new(Mutex::new(Vec::new())),
            shutdown_tx,
        }
    }

    /// Substitui a configuração padrão.
    pub fn with_config(mut self, config: StandaloneConfig) -> Self {
        self.config = config;
        self
    }

    /// Sinaliza graceful shutdown: o loop interrompe novas chamadas de
    /// receive e [`Self::run`] retorna após drenar todas as mensagens em voo.
    pub fn signal_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Long-poll loop: chama `ReceiveMessage` continuamente, despacha cada
    /// mensagem para os handlers inscritos e deleta as processadas com
    /// sucesso. Handlers com erro deixam a mensagem voltar pelo visibility
    /// timeout (ou são roteadas via DLQ pelo layer correspondente).
    pub async fn run(self: &Arc<Self>) -> Result<(), BrokerError> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut tasks: JoinSet<()> = JoinSet::new();

        loop {
            if *shutdown_rx.borrow() {
                break;
            }

            let recv = tokio::select! {
                biased;
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                    continue;
                }
                r = self.receive.receive(
                    &self.queue_url,
                    self.config.max_messages,
                    self.config.wait_time_seconds,
                ) => r,
            };

            match recv {
                Ok(result) => {
                    for msg in result.messages {
                        let this = self.clone();
                        tasks.spawn(async move {
                            this.dispatch_one(msg).await;
                        });
                    }
                }
                Err(e) => {
                    eprintln!("[serverust-events] sqs receive error: {e}");
                    if !self.config.error_backoff.is_zero() {
                        tokio::select! {
                            _ = tokio::time::sleep(self.config.error_backoff) => {}
                            _ = shutdown_rx.changed() => {}
                        }
                    }
                }
            }

            // Colhe tasks já concluídas sem bloquear.
            while tasks.try_join_next().is_some() {}
        }

        // Drena mensagens em voo antes de retornar (graceful shutdown).
        while tasks.join_next().await.is_some() {}
        Ok(())
    }

    async fn dispatch_one(&self, msg: SqsMessage) {
        let handlers: Vec<BoxedHandler> = self
            .subscriptions
            .lock()
            .expect("sqs subscriptions mutex poisoned")
            .iter()
            .filter(|s| s.queue == self.queue_name)
            .map(|s| s.handler.clone())
            .collect();

        if handlers.is_empty() {
            return;
        }

        let broker_msg = build_broker_message(&self.queue_name, &msg);

        let mut all_ok = true;
        for handler in handlers {
            if let Err(e) = handler(broker_msg.clone()).await {
                eprintln!(
                    "[serverust-events] sqs standalone handler error in queue {}: {e}",
                    self.queue_name
                );
                all_ok = false;
                break;
            }
        }

        if all_ok {
            if let Some(rh) = msg.receipt_handle.as_deref() {
                let id = msg.message_id.clone().unwrap_or_else(|| rh.to_string());
                let entry = DeleteEntry::new(id, rh.to_string());
                self.delete_manager
                    .delete_successful(&self.queue_url, vec![entry])
                    .await;
            }
        }
    }
}

#[async_trait]
impl<R, D> Broker for StandaloneSqsBroker<R, D>
where
    R: ReceiveClient,
    D: DeleteClient + 'static,
{
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
            "StandaloneSqsBroker is sink-only: use SqsProducer (US-004) to send messages"
                .to_string(),
        ))
    }
}
