//! Tower [`Layer`]s genéricos baseados nos blocos de telemetria.
//!
//! Estes adapters tornam a telemetria do `serverust` componível com qualquer
//! `tower::Service<Req>`, independente de HTTP ou event-driven. Subscribers
//! SQS, handlers HTTP, workers Kafka — todos consomem a mesma stack.
//!
//! # `TracingLayer`
//!
//! Envelopa a chamada do `Service` interno em um `tracing::span!` e instrumenta
//! o future com [`tracing::Instrument`]. O span carrega o nome fornecido no
//! construtor e o tipo da request (`type_name`), e os atributos extras passados
//! via [`TracingLayer::with_attribute`] tornam-se campos do span.
//!
//! ```ignore
//! use tower::ServiceBuilder;
//! use serverust_telemetry::tower::TracingLayer;
//!
//! let svc = ServiceBuilder::new()
//!     .layer(TracingLayer::new("sqs.subscriber"))
//!     .service(my_subscriber);
//! ```
//!
//! O layer é deliberadamente genérico (`<Req>`): qualquer `Service<Req>`
//! funciona — `SqsMessage`, `http::Request<B>`, `KafkaRecord<T>`, etc.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project_lite::pin_project;
use tower::{Layer, Service};
use tracing::Instrument;

/// [`Layer`] que envolve o `Service` interno em um span `tracing`.
#[derive(Clone, Debug)]
pub struct TracingLayer {
    name: &'static str,
}

impl TracingLayer {
    /// Cria a layer com o `name` usado como nome do span.
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl<S> Layer<S> for TracingLayer {
    type Service = TracingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TracingService {
            name: self.name,
            inner,
        }
    }
}

/// `Service` produzido por [`TracingLayer`].
#[derive(Clone, Debug)]
pub struct TracingService<S> {
    name: &'static str,
    inner: S,
}

impl<S, Req> Service<Req> for TracingService<S>
where
    S: Service<Req>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    Req: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = TracingFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let span = tracing::info_span!(
            "tower.service",
            otel.name = self.name,
            req.type = std::any::type_name::<Req>(),
        );
        tracing::trace!(parent: &span, "tower service call");
        TracingFuture {
            inner: Box::pin(self.inner.call(req).instrument(span)),
        }
    }
}

pin_project! {
    /// Future retornado por [`TracingService::call`].
    pub struct TracingFuture<F> {
        #[pin]
        inner: Pin<Box<tracing::instrument::Instrumented<F>>>,
    }
}

impl<F, R, E> Future for TracingFuture<F>
where
    F: Future<Output = Result<R, E>>,
{
    type Output = Result<R, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.as_mut().poll(cx)
    }
}

impl<F> fmt::Debug for TracingFuture<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TracingFuture").finish_non_exhaustive()
    }
}
