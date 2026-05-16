//! Detecção do runtime de execução (US-7).
//!
//! `Runtime::detect()` inspeciona a env `AWS_LAMBDA_FUNCTION_NAME` para
//! diferenciar AWS Lambda (driven pelo runtime AWS via `KafkaEvent` trigger)
//! de um processo long-running (ECS/EC2 com consumer loop).
//!
//! A API pública do [`EventRouter`](crate::router::EventRouter) é a mesma
//! em ambos os modos — só muda a implementação de
//! [`Broker`](crate::broker::Broker) injetada via `attach`:
//!
//! ```ignore
//! use std::sync::Arc;
//! use serverust_events::runtime::Runtime;
//! use serverust_events::router::EventRouter;
//!
//! # async fn example(router: EventRouter) -> Result<(), serverust_events::broker::BrokerError> {
//! match Runtime::detect() {
//!     Runtime::Lambda => {
//!         let broker = Arc::new(serverust_events::broker::lambda::LambdaBroker::new());
//!         router.attach(broker.clone()).await?;
//!         // ...lambda_runtime::run(|event| broker.handle_kafka_event(&event))...
//!     }
//!     Runtime::LongRunning => {
//!         // KafkaBroker exige a feature `kafka`.
//!         # /*
//!         let broker = Arc::new(KafkaBroker::from_env()?);
//!         router.attach(broker.clone()).await?;
//!         broker.run_consumer_loop().await?;
//!         # */
//!     }
//! }
//! # Ok(())
//! # }
//! ```

/// Variante de runtime detectada.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Runtime {
    /// Em execução dentro do runtime AWS Lambda.
    Lambda,
    /// Processo long-running (ECS, EC2, container, dev local).
    LongRunning,
}

impl Runtime {
    /// Detecta o runtime atual via env `AWS_LAMBDA_FUNCTION_NAME`.
    ///
    /// O runtime AWS Lambda sempre define essa variável; ausência significa
    /// que estamos rodando em outro lugar (long-running).
    pub fn detect() -> Self {
        if std::env::var_os("AWS_LAMBDA_FUNCTION_NAME").is_some() {
            Runtime::Lambda
        } else {
            Runtime::LongRunning
        }
    }

    /// `true` se o runtime detectado é AWS Lambda.
    pub fn is_lambda(&self) -> bool {
        matches!(self, Runtime::Lambda)
    }

    /// `true` se o runtime detectado é long-running.
    pub fn is_long_running(&self) -> bool {
        matches!(self, Runtime::LongRunning)
    }
}
