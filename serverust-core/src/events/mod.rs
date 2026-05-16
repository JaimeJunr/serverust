//! Suporte a event handlers tipados, paralelo ao roteador HTTP.
//!
//! [`EventHandler<E>`] permite plugar handlers para event sources não-HTTP
//! (Kafka, SQS, EventBridge, S3) sem afetar o roteamento HTTP existente.
//! O [`Container`](crate::Container) é compartilhado entre handlers HTTP e
//! event handlers, garantindo a mesma injeção de dependências.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::container::Container;

/// Erro retornado por [`EventHandler::handle`].
#[derive(Debug)]
pub struct EventError(pub String);

impl std::fmt::Display for EventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EventError: {}", self.0)
    }
}

impl std::error::Error for EventError {}

/// Handler para um tipo de evento `E`.
///
/// Implementadores recebem o evento por valor e o [`Container`] compartilhado
/// para injeção de dependências, e retornam `Ok(())` ou um [`EventError`].
///
/// # Exemplo
///
/// ```rust
/// use serverust_core::events::{EventHandler, EventError};
/// use serverust_core::Container;
/// use std::sync::Arc;
///
/// struct MyEvent { value: u32 }
///
/// struct MyHandler;
///
/// impl EventHandler<MyEvent> for MyHandler {
///     async fn handle(&self, event: MyEvent, _ctx: &Container) -> Result<(), EventError> {
///         println!("received: {}", event.value);
///         Ok(())
///     }
/// }
/// ```
pub trait EventHandler<E: Send + 'static>: Send + Sync + 'static {
    fn handle(
        &self,
        event: E,
        ctx: &Container,
    ) -> impl Future<Output = Result<(), EventError>> + Send;
}

// Trait interna apagada para permitir boxing de handlers heterogêneos.
pub(crate) trait ErasedHandler<E: Send + 'static>: Send + Sync {
    fn handle_erased<'a>(
        &'a self,
        event: E,
        ctx: &'a Container,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventError>> + Send + 'a>>;
}

struct HandlerWrapper<H>(H);

impl<E, H> ErasedHandler<E> for HandlerWrapper<H>
where
    E: Send + 'static,
    H: EventHandler<E>,
{
    fn handle_erased<'a>(
        &'a self,
        event: E,
        ctx: &'a Container,
    ) -> Pin<Box<dyn Future<Output = Result<(), EventError>> + Send + 'a>> {
        Box::pin(self.0.handle(event, ctx))
    }
}

/// Despacha eventos `E` para os handlers registrados via [`crate::App::event`].
///
/// Constrói-se via [`crate::App::into_event_dispatcher`] e executa os
/// handlers em sequência, parando no primeiro erro.
pub struct EventDispatcher<E: Send + 'static> {
    handlers: Vec<Arc<dyn ErasedHandler<E>>>,
    container: Container,
}

impl<E: Send + 'static> EventDispatcher<E> {
    pub(crate) fn new(handlers: Vec<Arc<dyn ErasedHandler<E>>>, container: Container) -> Self {
        Self {
            handlers,
            container,
        }
    }

    /// Despacha `event` para todos os handlers registrados em sequência.
    /// Retorna no primeiro erro encontrado.
    pub async fn dispatch_event(&self, event: E) -> Result<(), EventError>
    where
        E: Clone,
    {
        let last = self.handlers.len().saturating_sub(1);
        for (i, handler) in self.handlers.iter().enumerate() {
            let evt = if i == last {
                // Último handler: consume sem clone extra (mas E: Clone já existe)
                event.clone()
            } else {
                event.clone()
            };
            handler.handle_erased(evt, &self.container).await?;
        }
        Ok(())
    }
}

/// Builder interno que acumula handlers antes de construir o EventDispatcher.
pub(crate) struct EventHandlerRegistry<E: Send + 'static> {
    handlers: Vec<Arc<dyn ErasedHandler<E>>>,
}

impl<E: Send + 'static> EventHandlerRegistry<E> {
    pub(crate) fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub(crate) fn register<H: EventHandler<E>>(&mut self, handler: H) {
        self.handlers.push(Arc::new(HandlerWrapper(handler)));
    }

    pub(crate) fn into_dispatcher(self, container: Container) -> EventDispatcher<E> {
        EventDispatcher::new(self.handlers, container)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone)]
    struct TestEvent {
        value: u32,
    }

    struct RecordingHandler {
        log: Arc<Mutex<Vec<u32>>>,
    }

    impl EventHandler<TestEvent> for RecordingHandler {
        async fn handle(&self, event: TestEvent, _ctx: &Container) -> Result<(), EventError> {
            self.log.lock().unwrap().push(event.value);
            Ok(())
        }
    }

    struct StateCheckHandler;

    impl EventHandler<TestEvent> for StateCheckHandler {
        async fn handle(&self, event: TestEvent, ctx: &Container) -> Result<(), EventError> {
            let counter: Arc<Mutex<u32>> = ctx
                .get::<Mutex<u32>>()
                .ok_or_else(|| EventError("counter not found".into()))?;
            let mut v = counter.lock().unwrap();
            *v += event.value;
            Ok(())
        }
    }

    struct FailingHandler;

    impl EventHandler<TestEvent> for FailingHandler {
        async fn handle(&self, _event: TestEvent, _ctx: &Container) -> Result<(), EventError> {
            Err(EventError("intentional failure".into()))
        }
    }

    #[tokio::test]
    async fn handler_stub_dispara_dispatch_event_e_verifica_retorno() {
        let log = Arc::new(Mutex::new(Vec::<u32>::new()));
        let mut registry = EventHandlerRegistry::<TestEvent>::new();
        registry.register(RecordingHandler {
            log: Arc::clone(&log),
        });

        let dispatcher = registry.into_dispatcher(Container::new());
        dispatcher
            .dispatch_event(TestEvent { value: 42 })
            .await
            .unwrap();

        assert_eq!(*log.lock().unwrap(), vec![42]);
    }

    #[tokio::test]
    async fn handler_acessa_state_injetado_no_container() {
        let counter = Arc::new(Mutex::new(0u32));
        let mut container = Container::new();
        container.insert::<Mutex<u32>>(Arc::clone(&counter));

        let mut registry = EventHandlerRegistry::<TestEvent>::new();
        registry.register(StateCheckHandler);

        let dispatcher = registry.into_dispatcher(container);
        dispatcher
            .dispatch_event(TestEvent { value: 10 })
            .await
            .unwrap();

        assert_eq!(*counter.lock().unwrap(), 10);
    }

    #[tokio::test]
    async fn multiplos_handlers_executam_em_sequencia() {
        let log = Arc::new(Mutex::new(Vec::<u32>::new()));
        let log2 = Arc::clone(&log);
        let mut registry = EventHandlerRegistry::<TestEvent>::new();
        registry.register(RecordingHandler {
            log: Arc::clone(&log),
        });
        registry.register(RecordingHandler { log: log2 });

        let dispatcher = registry.into_dispatcher(Container::new());
        dispatcher
            .dispatch_event(TestEvent { value: 7 })
            .await
            .unwrap();

        assert_eq!(*log.lock().unwrap(), vec![7, 7]);
    }

    #[tokio::test]
    async fn dispatch_para_no_primeiro_erro() {
        let log = Arc::new(Mutex::new(Vec::<u32>::new()));
        let mut registry = EventHandlerRegistry::<TestEvent>::new();
        registry.register(FailingHandler);
        registry.register(RecordingHandler {
            log: Arc::clone(&log),
        });

        let dispatcher = registry.into_dispatcher(Container::new());
        let result = dispatcher.dispatch_event(TestEvent { value: 1 }).await;

        assert!(result.is_err());
        assert!(
            log.lock().unwrap().is_empty(),
            "segundo handler não deve ter rodado"
        );
    }
}
