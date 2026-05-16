//! `EventRouter` — builder programático para inscrever handlers tipados em
//! qualquer implementação de [`crate::broker::Broker`].
//!
//! API alvo (PRD §6):
//!
//! ```ignore
//! use std::time::Duration;
//! use serverust_events::broker::Broker;
//! use serverust_events::router::EventRouter;
//! use serverust_events::retry::RetryPolicy;
//!
//! # async fn handle_order(_e: ()) -> Result<(), serverust_events::broker::BrokerError> { Ok(()) }
//! # async fn example(broker: impl Broker) -> Result<(), serverust_events::broker::BrokerError> {
//! EventRouter::new()
//!     .subscribe::<(), _, _>("orders.created", handle_order)
//!     .with_retry(RetryPolicy::exponential(3, Duration::from_secs(1)))
//!     .with_dlq("orders.dlq")
//!     .attach(&broker)
//!     .await
//! # }
//! ```
//!
//! O builder grava as inscrições; a entrega real depende do `Broker`
//! injetado em [`EventRouter::attach`]. A lógica de retry e a publicação
//! efetiva no DLQ chegam em US-5 — por ora, os campos `retry` e `dlq`
//! ficam disponíveis para inspeção e são consumidos em iterações futuras.

use std::any::Any;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::broker::{BoxedHandler, Broker, BrokerError, BrokerMessage};
use crate::extract::HandlerFn;
use crate::retry::RetryPolicy;

/// Inscrição registrada no router: tópico, handler boxado e configuração
/// opcional de retry/DLQ.
///
/// `retry` e `dlq` ainda não são consumidos em runtime — a aplicação
/// chega em US-5. Mantidos `pub(crate)` para uso futuro.
pub(crate) struct Subscription {
    pub(crate) topic: String,
    pub(crate) handler: BoxedHandler,
    #[allow(dead_code)]
    pub(crate) retry: Option<RetryPolicy>,
    #[allow(dead_code)]
    pub(crate) dlq: Option<String>,
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
    /// Aceita qualquer `impl Broker`, incluindo trait objects
    /// (`&dyn Broker`, `Arc<dyn Broker>` via `as_ref`).
    pub async fn attach<B>(self, broker: &B) -> Result<(), BrokerError>
    where
        B: Broker + ?Sized,
    {
        for sub in self.subscriptions {
            broker.subscribe(&sub.topic, sub.handler).await?;
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

    /// Retorna o tópico DLQ associado à última inscrição, se houver.
    pub fn last_dlq(&self) -> Option<&str> {
        self.subscriptions.last().and_then(|s| s.dlq.as_deref())
    }
}
