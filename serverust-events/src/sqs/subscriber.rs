//! `SqsSubscriber` — implementação [`tower::Service<SqsMessage>`] para compor
//! subscribers SQS em uma stack [`tower::ServiceBuilder`].
//!
//! O subscriber recebe uma função de despacho assíncrona
//! (`FnMut(SqsMessage) -> Future<Output = Result<(), BrokerError>>`) e expõe
//! a mesma como um `Service<SqsMessage, Response = (), Error = BrokerError>`,
//! permitindo que qualquer layer Tower seja aplicado em cima dela:
//!
//! ```ignore
//! use std::sync::Arc;
//! use tower::ServiceBuilder;
//! use serverust_events::sqs::subscriber::SqsSubscriber;
//! use serverust_telemetry::tower::TracingLayer;
//!
//! let svc = ServiceBuilder::new()
//!     .layer(TracingLayer::new("sqs.orders"))
//!     .service(SqsSubscriber::new(|msg| async move {
//!         // negócio
//!         Ok(())
//!     }));
//! ```
//!
//! O layer/service é genérico no payload — a `SqsMessage` chega in-natura
//! (vinda de [`super::consumer::SqsBroker::handle_sqs_event`] ou de um worker
//! standalone, US-010), e o middleware pode inspecioná-la antes do handler.
//!
//! # Ordem de composição recomendada
//!
//! Tower aplica layers no estilo "newer is outer": o último `.layer(...)` é o
//! mais externo. A ordem default sugerida pelo PRD §6.3 é:
//!
//! ```text
//! inbound → TracingLayer → IdempotencyLayer → RetryLayer → handler
//! ```
//!
//! Em `ServiceBuilder`, isso se traduz para o tracing aplicado **por último**
//! (newer outer); na prática:
//!
//! ```ignore
//! ServiceBuilder::new()
//!     .layer(TracingLayer::new(...))        // outermost (tracing fora)
//!     .layer(IdempotencyLayer::new(store))  // meio
//!     .layer(RetryLayer::new(policy))       // innermost (retry dentro)
//!     .service(subscriber)
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use aws_lambda_events::event::sqs::SqsMessage;
use tower::Service;

use crate::broker::BrokerError;

/// Tipo da função de despacho usada por [`SqsSubscriber`].
type DispatchFn = Arc<
    dyn Fn(SqsMessage) -> Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>>
        + Send
        + Sync,
>;

/// `Service<SqsMessage, Response = (), Error = BrokerError>` que despacha cada
/// mensagem para uma função `async` registrada na construção.
///
/// Clonar é barato (`Arc` interno), tornando o subscriber compatível com
/// Tower `ServiceBuilder` e `tower::util::BoxCloneService`.
#[derive(Clone)]
pub struct SqsSubscriber {
    dispatch: DispatchFn,
}

impl SqsSubscriber {
    /// Cria um subscriber a partir de uma função `async`.
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(SqsMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), BrokerError>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        Self {
            dispatch: Arc::new(move |msg| {
                let handler = handler.clone();
                Box::pin(async move { handler(msg).await })
            }),
        }
    }
}

impl std::fmt::Debug for SqsSubscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqsSubscriber").finish_non_exhaustive()
    }
}

impl Service<SqsMessage> for SqsSubscriber {
    type Response = ();
    type Error = BrokerError;
    type Future = Pin<Box<dyn Future<Output = Result<(), BrokerError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: SqsMessage) -> Self::Future {
        (self.dispatch)(req)
    }
}
