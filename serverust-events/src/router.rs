//! `EventRouter` — builder programático para inscrever handlers tipados em
//! qualquer implementação de [`crate::broker::Broker`].
//!
//! API alvo (PRD §6):
//!
//! ```ignore
//! use std::sync::Arc;
//! use std::time::Duration;
//! use serverust_events::broker::Broker;
//! use serverust_events::router::EventRouter;
//! use serverust_events::retry::RetryPolicy;
//!
//! # async fn handle_order(_e: ()) -> Result<(), serverust_events::broker::BrokerError> { Ok(()) }
//! # async fn example(broker: impl Broker + 'static) -> Result<(), serverust_events::broker::BrokerError> {
//! let broker = Arc::new(broker);
//! EventRouter::new()
//!     .subscribe::<(), _, _>("orders.created", handle_order)
//!     .with_retry(RetryPolicy::exponential(3, Duration::from_secs(1)))
//!     .with_dlq("orders.dlq")
//!     .attach(broker)
//!     .await
//! # }
//! ```
//!
//! O builder grava as inscrições; a entrega real depende do `Broker`
//! injetado em [`EventRouter::attach`]. Subscriptions com `retry`/`dlq`
//! configurados têm o handler envolvido em wrapper de retry automático.

use std::any::Any;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::broker::{BoxedHandler, Broker, BrokerError, BrokerMessage};
use crate::extract::HandlerFn;
use crate::retry::RetryPolicy;

pub(crate) struct Subscription {
    pub(crate) topic: String,
    pub(crate) handler: BoxedHandler,
    pub(crate) retry: Option<RetryPolicy>,
    pub(crate) dlq: Option<String>,
}

/// Envolve `handler` com loop de retry e publicação em DLQ após esgotamento.
fn wrap_with_retry<B>(
    handler: BoxedHandler,
    retry: Option<RetryPolicy>,
    dlq: Option<String>,
    broker: Arc<B>,
) -> BoxedHandler
where
    B: Broker + ?Sized + 'static,
{
    Arc::new(move |msg: BrokerMessage| {
        let handler = handler.clone();
        let retry = retry.clone();
        let dlq = dlq.clone();
        let broker = broker.clone();
        Box::pin(async move {
            let max_attempts = retry.as_ref().map(|r| r.max_attempts()).unwrap_or(1).max(1);
            let mut last_err: Option<BrokerError> = None;

            for attempt in 0..max_attempts {
                if attempt > 0 {
                    if let Some(RetryPolicy::Exponential { base_delay, .. }) = &retry {
                        let delay = *base_delay * 2u32.pow(attempt - 1);
                        tokio::time::sleep(delay).await;
                    }
                }
                match handler(msg.clone()).await {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
            }

            // Todas as tentativas falharam — publicar no DLQ se configurado.
            if let Some(dlq_topic) = &dlq {
                let _ = broker.publish(dlq_topic, &msg.payload).await;
            }
            Err(last_err.unwrap_or_else(|| BrokerError::Subscribe("sem tentativas".to_string())))
        })
    })
}

/// Router event-driven. Acumula inscrições e aplica todas a um broker
/// concreto via [`EventRouter::attach`].
#[derive(Default)]
pub struct EventRouter {
    subscriptions: Vec<Subscription>,
    state: Option<Arc<dyn Any + Send + Sync>>,
}

impl EventRouter {
    /// Cria um router vazio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um handler tipado para `topic`.
    ///
    /// O payload é desserializado de JSON para `T` antes de invocar
    /// `handler`. Falha de desserialização retorna
    /// [`BrokerError::Subscribe`] sem chamar `handler`.
    pub fn subscribe<T, H, Fut>(mut self, topic: &str, handler: H) -> Self
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), BrokerError>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        let wrapped: BoxedHandler = Arc::new(move |msg: BrokerMessage| {
            let handler = handler.clone();
            Box::pin(async move {
                let event: T = serde_json::from_slice(&msg.payload)
                    .map_err(|e| BrokerError::Subscribe(format!("payload decode error: {e}")))?;
                handler(event).await
            })
        });

        self.subscriptions.push(Subscription {
            topic: topic.to_string(),
            handler: wrapped,
            retry: None,
            dlq: None,
        });
        self
    }

    /// Registra estado compartilhado injetável nos handlers via `State<S>`.
    pub fn with_state<S: Any + Send + Sync + 'static>(mut self, state: S) -> Self {
        self.state = Some(Arc::new(state));
        self
    }

    /// Registra um handler com extractors tipados para `topic`.
    ///
    /// O handler pode declarar zero ou mais extractors além do evento `T`:
    /// [`crate::extract::EventCtx`], [`crate::extract::KafkaHeaders`],
    /// [`crate::extract::State<S>`].
    ///
    /// # Exemplo
    ///
    /// ```ignore
    /// router.subscribe_with("orders", |event: Order, ctx: EventCtx| async { Ok(()) });
    /// ```
    pub fn subscribe_with<T, Exts, H>(mut self, topic: &str, handler: H) -> Self
    where
        T: DeserializeOwned + Send + 'static,
        H: HandlerFn<T, Exts>,
    {
        let state = self.state.clone();
        let wrapped: BoxedHandler = Arc::new(move |msg: BrokerMessage| {
            let handler = handler.clone();
            let state = state.clone();
            handler.call(msg, state)
        });

        self.subscriptions.push(Subscription {
            topic: topic.to_string(),
            handler: wrapped,
            retry: None,
            dlq: None,
        });
        self
    }

    /// Aplica `policy` à última inscrição registrada. No-op se nenhum
    /// `subscribe` foi chamado antes.
    pub fn with_retry(mut self, policy: RetryPolicy) -> Self {
        if let Some(last) = self.subscriptions.last_mut() {
            last.retry = Some(policy);
        }
        self
    }

    /// Define o tópico DLQ da última inscrição. No-op se nenhum
    /// `subscribe` foi chamado antes.
    pub fn with_dlq(mut self, topic: impl Into<String>) -> Self {
        if let Some(last) = self.subscriptions.last_mut() {
            last.dlq = Some(topic.into());
        }
        self
    }

    /// Registra todas as inscrições no `broker` fornecido.
    ///
    /// Subscriptions com `retry` ou `dlq` configurados têm o handler
    /// automaticamente envolvido no loop de retry (com backoff exponencial
    /// quando aplicável) e publicação no DLQ após esgotamento.
    ///
    /// Aceita `Arc<ConcreteType>` e `Arc<dyn Broker>`.
    pub async fn attach<B>(self, broker: Arc<B>) -> Result<(), BrokerError>
    where
        B: Broker + ?Sized + 'static,
    {
        for sub in self.subscriptions {
            // Calcula o DLQ efetivo: `with_dlq` tem precedência sobre `dead_letter` na policy.
            let effective_dlq = sub.dlq.clone().or_else(|| {
                sub.retry
                    .as_ref()
                    .and_then(|r| r.dlq_topic().map(str::to_string))
            });

            let handler = if sub.retry.is_some() || effective_dlq.is_some() {
                wrap_with_retry(sub.handler, sub.retry, effective_dlq, broker.clone())
            } else {
                sub.handler
            };
            broker.subscribe(&sub.topic, handler).await?;
        }
        Ok(())
    }

    // ---- introspecção para testes / debugging --------------------------

    /// Lista os tópicos inscritos na ordem em que foram adicionados.
    pub fn subscription_topics(&self) -> Vec<String> {
        self.subscriptions.iter().map(|s| s.topic.clone()).collect()
    }

    /// Retorna a [`RetryPolicy`] associada à última inscrição, se houver.
    pub fn last_retry(&self) -> Option<&RetryPolicy> {
        self.subscriptions.last().and_then(|s| s.retry.as_ref())
    }

    /// Retorna o tópico DLQ efetivo da última inscrição (via `with_dlq` ou `dead_letter`).
    pub fn last_dlq(&self) -> Option<&str> {
        self.subscriptions.last().and_then(|s| {
            s.dlq
                .as_deref()
                .or_else(|| s.retry.as_ref().and_then(|r| r.dlq_topic()))
        })
    }
}
