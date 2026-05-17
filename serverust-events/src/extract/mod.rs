//! Extractors tipados para handlers event-driven (US-4).
//!
//! Permite que handlers declarem dependências como parâmetros de função:
//!
//! ```ignore
//! use serverust_events::extract::{EventCtx, KafkaHeaders, State};
//!
//! async fn my_handler(
//!     event: MyEvent,
//!     ctx: EventCtx,
//!     headers: KafkaHeaders,
//!     state: State<AppState>,
//! ) -> Result<(), serverust_events::broker::BrokerError> {
//!     println!("topic={}, state={:?}", ctx.topic, state.0);
//!     Ok(())
//! }
//! ```
//!
//! Registre o handler via [`crate::router::EventRouter::subscribe_with`].

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::broker::{BrokerError, BrokerMessage, HandlerFuture};

/// Trait implementada por tipos que podem ser extraídos de um [`BrokerMessage`].
///
/// Distinta de `DeserializeOwned` para evitar conflito de impl: `FromExtractor`
/// só deve ser implementada para tipos de metadados/estado, enquanto o evento
/// `T: DeserializeOwned` é desserializado diretamente do payload.
pub trait FromExtractor: Sized + Send + 'static {
    fn from_message(
        msg: &BrokerMessage,
        state: Option<&Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self, BrokerError>;
}

// ---------------------------------------------------------------------------
// Extractors concretos
// ---------------------------------------------------------------------------

/// Wrapper explícito que desserializa o payload do evento como JSON.
///
/// Análogo ao extractor `Json<T>` do Axum: permite que handlers declarem
/// explicitamente que recebem o corpo da mensagem como `T`.
///
/// - Como primeiro argumento (`T`): desserializado via `serde_json` do payload.
/// - Como extractor posicional (`E1`, `E2`, `E3`): implementa [`FromExtractor`],
///   também desserializa do payload.
///
/// # Exemplo
///
/// ```ignore
/// async fn handle(Json(order): Json<Order>, meta: SqsMetadata) -> Result<(), BrokerError> {
///     println!("order_id={}", order.order_id);
///     Ok(())
/// }
/// ```
pub struct Json<T>(pub T);

impl<T: std::ops::Deref> std::ops::Deref for Json<T> {
    type Target = T::Target;
    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Json<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        T::deserialize(d).map(Json)
    }
}

impl<T: DeserializeOwned + Send + 'static> FromExtractor for Json<T> {
    fn from_message(
        msg: &BrokerMessage,
        _state: Option<&Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self, BrokerError> {
        serde_json::from_slice(&msg.payload).map(Json).map_err(|e| {
            BrokerError::Subscribe(format!("Json extractor: payload decode error: {e}"))
        })
    }
}

/// Metadados do evento: tópico, partição, offset e timestamp.
pub struct EventCtx {
    pub topic: String,
    pub partition: Option<i64>,
    pub offset: Option<i64>,
    pub timestamp: Option<i64>,
}

impl FromExtractor for EventCtx {
    fn from_message(
        msg: &BrokerMessage,
        _state: Option<&Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self, BrokerError> {
        Ok(EventCtx {
            topic: msg.topic.clone(),
            partition: msg.partition,
            offset: msg.offset,
            timestamp: msg.timestamp,
        })
    }
}

/// Headers do registro como mapa `nome → bytes`.
pub struct KafkaHeaders(pub HashMap<String, Vec<u8>>);

impl FromExtractor for KafkaHeaders {
    fn from_message(
        msg: &BrokerMessage,
        _state: Option<&Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self, BrokerError> {
        Ok(KafkaHeaders(msg.headers.clone()))
    }
}

/// Estado compartilhado injetado no handler.
///
/// Requer registro prévio via [`crate::router::EventRouter::with_state`].
/// Derrefa para `S` via `Deref`.
pub struct State<S>(pub Arc<S>);

impl<S> std::ops::Deref for State<S> {
    type Target = S;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync + 'static> FromExtractor for State<S> {
    fn from_message(
        _msg: &BrokerMessage,
        state: Option<&Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self, BrokerError> {
        let state = state.ok_or_else(|| {
            BrokerError::Subscribe("State extractor requires with_state()".into())
        })?;
        let s = Arc::clone(state)
            .downcast::<S>()
            .map_err(|_| BrokerError::Subscribe("state type mismatch".into()))?;
        Ok(State(s))
    }
}

// ---------------------------------------------------------------------------
// Trait HandlerFn<T, Extractors>
// ---------------------------------------------------------------------------

/// Trait para handlers que aceitam um evento `T` mais zero ou mais extractors.
///
/// Implementado automaticamente para funções `Fn(T) -> Fut`,
/// `Fn(T, E1) -> Fut`, `Fn(T, E1, E2) -> Fut`, e `Fn(T, E1, E2, E3) -> Fut`
/// onde `T: DeserializeOwned` e `E1, E2, E3: FromExtractor`.
pub trait HandlerFn<T, Exts>: Clone + Send + Sync + 'static
where
    T: DeserializeOwned + Send + 'static,
{
    fn call(self, msg: BrokerMessage, state: Option<Arc<dyn Any + Send + Sync>>) -> HandlerFuture;
}

// ---------------------------------------------------------------------------
// 0 extractors: Fn(T) -> Fut
// ---------------------------------------------------------------------------

impl<F, Fut, T> HandlerFn<T, ()> for F
where
    F: Fn(T) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(), BrokerError>> + Send + 'static,
    T: DeserializeOwned + Send + 'static,
{
    fn call(self, msg: BrokerMessage, _state: Option<Arc<dyn Any + Send + Sync>>) -> HandlerFuture {
        Box::pin(async move {
            let event: T = serde_json::from_slice(&msg.payload)
                .map_err(|e| BrokerError::Subscribe(format!("payload decode error: {e}")))?;
            self(event).await
        })
    }
}

// ---------------------------------------------------------------------------
// 1-3 extractors via macro
// ---------------------------------------------------------------------------

macro_rules! impl_handler_fn {
    ($first:ident $(, $rest:ident)*) => {
        #[allow(non_snake_case)]
        impl<F, Fut, T, $first $(, $rest)*> HandlerFn<T, ($first, $($rest,)*)> for F
        where
            F: Fn(T, $first $(, $rest)*) -> Fut + Clone + Send + Sync + 'static,
            Fut: std::future::Future<Output = Result<(), BrokerError>> + Send + 'static,
            T: DeserializeOwned + Send + 'static,
            $first: FromExtractor,
            $($rest: FromExtractor,)*
        {
            fn call(self, msg: BrokerMessage, state: Option<Arc<dyn Any + Send + Sync>>) -> HandlerFuture {
                Box::pin(async move {
                    let event: T = serde_json::from_slice(&msg.payload)
                        .map_err(|e| BrokerError::Subscribe(format!("payload decode error: {e}")))?;
                    let $first = $first::from_message(&msg, state.as_ref())?;
                    $(let $rest = $rest::from_message(&msg, state.as_ref())?;)*
                    self(event, $first $(, $rest)*).await
                })
            }
        }
    };
}

impl_handler_fn!(E1);
impl_handler_fn!(E1, E2);
impl_handler_fn!(E1, E2, E3);
