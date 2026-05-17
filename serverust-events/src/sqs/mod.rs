//! Adapter SQS para `serverust-events` (US-001).
//!
//! Atrás de `feature = "sqs"`: o módulo só é compilado quando a feature está
//! habilitada, garantindo que `serverust-core` e exemplos como `hello-world`
//! não puxam código nem dependências de SQS.
//!
//! Submódulos:
//!
//! - [`consumer`] — [`consumer::SqsBroker`] (sink-only, Lambda ESM): registra
//!   handlers por nome de fila e despacha um `SqsEvent` em uma
//!   `SqsBatchResponse` com `batch_item_failures` para mensagens em erro.
//! - [`extract`] — extractors específicos do SQS, como [`extract::SqsMetadata`].

pub mod consumer;
pub mod extract;
