//! Extractors específicos do adapter SQS.
//!
//! [`SqsMetadata`] expõe os campos do [`aws_lambda_events::event::sqs::SqsMessage`]
//! original — `message_id`, `receipt_handle`, `attributes` (system attrs) e
//! `message_attributes` (user attrs).
//!
//! O extractor é implementado em cima do trait genérico
//! [`crate::extract::FromExtractor`]: o [`super::consumer::SqsBroker`] empacota
//! a `SqsMessage` original em um header bem-conhecido do `BrokerMessage`, e
//! este extractor decodifica de volta sob demanda. Usuários do framework
//! consomem apenas a struct pública abaixo.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use aws_lambda_events::event::sqs::{SqsMessage, SqsMessageAttribute};

use crate::broker::{BrokerError, BrokerMessage};
use crate::extract::FromExtractor;

use super::consumer::SQS_METADATA_HEADER;

/// Metadados de uma mensagem SQS — expostos como extractor em handlers
/// declarados com `#[subscriber(driver = "sqs", queue = "...")]`.
///
/// Mantém os campos mais usados de [`SqsMessage`]; a versão completa pode ser
/// recuperada via [`Self::message_id`] + lookup explícito quando necessário.
#[derive(Debug, Clone)]
pub struct SqsMetadata {
    pub message_id: String,
    pub receipt_handle: String,
    /// System attributes (`ApproximateReceiveCount`, `SentTimestamp`, etc.).
    pub attributes: HashMap<String, String>,
    /// User-defined message attributes (tipadas via `dataType`).
    pub message_attributes: HashMap<String, SqsMessageAttribute>,
}

impl FromExtractor for SqsMetadata {
    fn from_message(
        msg: &BrokerMessage,
        _state: Option<&Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self, BrokerError> {
        let raw = msg.headers.get(SQS_METADATA_HEADER).ok_or_else(|| {
            BrokerError::Subscribe(
                "SqsMetadata: BrokerMessage was not produced by SqsBroker (header ausente)".into(),
            )
        })?;
        let parsed: SqsMessage = serde_json::from_slice(raw).map_err(|e| {
            BrokerError::Subscribe(format!("SqsMetadata: failed to decode metadata: {e}"))
        })?;

        Ok(SqsMetadata {
            message_id: parsed.message_id.unwrap_or_default(),
            receipt_handle: parsed.receipt_handle.unwrap_or_default(),
            attributes: parsed.attributes,
            message_attributes: parsed.message_attributes,
        })
    }
}

/// Metadados específicos de uma mensagem de fila **SQS FIFO**.
///
/// Expõe os atributos exclusivos do modo FIFO:
///
/// - `message_group_id`: identifica o grupo de ordering (obrigatório em FIFO).
/// - `message_deduplication_id`: identificador de deduplicação (presente quando
///   enviado explicitamente ou via content-based dedupe).
/// - `sequence_number`: número de sequência atribuído pelo SQS na entrega.
///
/// O extractor falha em runtime se a mensagem **não** vier de uma fila FIFO
/// (sem `MessageGroupId` nos `attributes`). O compile-time guard que evita esse
/// erro está na macro `#[subscriber(driver = "sqs", queue = "...", fifo)]`:
/// ela exige que o handler declare `SqsFifoMetadata` quando `fifo` está
/// presente, e proíbe o tipo em queues standard.
#[derive(Debug, Clone)]
pub struct SqsFifoMetadata {
    /// Identificador do grupo de ordering FIFO (obrigatório).
    pub message_group_id: String,
    /// Identificador de deduplicação (opcional — content-based pode preencher).
    pub message_deduplication_id: Option<String>,
    /// Número de sequência atribuído pelo SQS.
    pub sequence_number: Option<String>,
}

impl FromExtractor for SqsFifoMetadata {
    fn from_message(
        msg: &BrokerMessage,
        _state: Option<&Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self, BrokerError> {
        let raw = msg.headers.get(SQS_METADATA_HEADER).ok_or_else(|| {
            BrokerError::Subscribe(
                "SqsFifoMetadata: BrokerMessage was not produced by SqsBroker (header ausente)"
                    .into(),
            )
        })?;
        let parsed: SqsMessage = serde_json::from_slice(raw).map_err(|e| {
            BrokerError::Subscribe(format!("SqsFifoMetadata: failed to decode metadata: {e}"))
        })?;

        let message_group_id = parsed.attributes.get("MessageGroupId").ok_or_else(|| {
            BrokerError::Subscribe(
                "SqsFifoMetadata: MessageGroupId ausente — a fila não é FIFO ou \
                 o atributo não foi propagado pelo Lambda ESM"
                    .into(),
            )
        })?;

        Ok(SqsFifoMetadata {
            message_group_id: message_group_id.clone(),
            message_deduplication_id: parsed.attributes.get("MessageDeduplicationId").cloned(),
            sequence_number: parsed.attributes.get("SequenceNumber").cloned(),
        })
    }
}
