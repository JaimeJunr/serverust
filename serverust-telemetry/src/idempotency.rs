//! Trait [`IdempotencyStore`] e implementações disponíveis no MVP.
//!
//! No MVP entregamos:
//! - [`IdempotencyStore`] como trait pública para usuários plugarem storage
//!   custom (Redis, Postgres, in-memory…).
//! - [`InMemoryIdempotencyStore`] como implementação de referência útil em
//!   testes e dev local.
//! - `DynamoDbIdempotencyStore` (atrás da feature `dynamodb`) usando
//!   `aws-sdk-dynamodb` — entrega "default DynamoDB" sem inflar o binário
//!   no build default.
//!
//! A macro `#[idempotent]` (decorator ergonômico ao redor da trait) fica
//! para a fase 2, conforme PRD §5.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Registro persistido por chave de idempotência.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub key: String,
    pub response_body: Vec<u8>,
    pub status_code: u16,
    /// Timestamp epoch ms para suportar TTL no storage destino.
    pub created_at_ms: u64,
}

/// Erros padronizados das operações de idempotência.
#[derive(Debug, thiserror::Error)]
pub enum IdempotencyError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("conflict: key already in flight")]
    Conflict,
}

/// Contrato implementado pelos storages.
#[async_trait]
pub trait IdempotencyStore: Send + Sync + 'static {
    /// Retorna o registro existente ou `None` se ainda não houve resposta.
    async fn get(&self, key: &str) -> Result<Option<IdempotencyRecord>, IdempotencyError>;

    /// Persiste o registro (sobrescreve se existir).
    async fn put(&self, record: IdempotencyRecord) -> Result<(), IdempotencyError>;
}

/// Implementação in-memory thread-safe — adequada para testes e para o
/// `serverust dev` local. Não persiste após restart.
#[derive(Debug, Default)]
pub struct InMemoryIdempotencyStore {
    inner: Mutex<HashMap<String, IdempotencyRecord>>,
}

impl InMemoryIdempotencyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn get(&self, key: &str) -> Result<Option<IdempotencyRecord>, IdempotencyError> {
        let guard = self
            .inner
            .lock()
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
        Ok(guard.get(key).cloned())
    }

    async fn put(&self, record: IdempotencyRecord) -> Result<(), IdempotencyError> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
        guard.insert(record.key.clone(), record);
        Ok(())
    }
}

#[cfg(feature = "dynamodb")]
mod dynamodb_impl {
    use super::{IdempotencyError, IdempotencyRecord, IdempotencyStore};
    use async_trait::async_trait;
    use aws_sdk_dynamodb::Client;
    use aws_sdk_dynamodb::primitives::Blob;
    use aws_sdk_dynamodb::types::AttributeValue;

    /// Implementação DynamoDB padrão. O usuário fornece a tabela; o schema
    /// esperado é `pk = correlation_key (S)` com itens contendo
    /// `response_body (B)`, `status_code (N)`, `created_at_ms (N)`.
    pub struct DynamoDbIdempotencyStore {
        client: Client,
        table_name: String,
    }

    impl DynamoDbIdempotencyStore {
        pub fn new(client: Client, table_name: impl Into<String>) -> Self {
            Self {
                client,
                table_name: table_name.into(),
            }
        }
    }

    #[async_trait]
    impl IdempotencyStore for DynamoDbIdempotencyStore {
        async fn get(&self, key: &str) -> Result<Option<IdempotencyRecord>, IdempotencyError> {
            let output = self
                .client
                .get_item()
                .table_name(&self.table_name)
                .key("pk", AttributeValue::S(key.to_string()))
                .send()
                .await
                .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

            let Some(item) = output.item else {
                return Ok(None);
            };
            let response_body = item
                .get("response_body")
                .and_then(|v| v.as_b().ok())
                .map(|b: &Blob| b.as_ref().to_vec())
                .unwrap_or_default();
            let status_code = item
                .get("status_code")
                .and_then(|v| v.as_n().ok())
                .and_then(|n| n.parse::<u16>().ok())
                .unwrap_or(200);
            let created_at_ms = item
                .get("created_at_ms")
                .and_then(|v| v.as_n().ok())
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0);

            Ok(Some(IdempotencyRecord {
                key: key.to_string(),
                response_body,
                status_code,
                created_at_ms,
            }))
        }

        async fn put(&self, record: IdempotencyRecord) -> Result<(), IdempotencyError> {
            self.client
                .put_item()
                .table_name(&self.table_name)
                .item("pk", AttributeValue::S(record.key.clone()))
                .item(
                    "response_body",
                    AttributeValue::B(Blob::new(record.response_body)),
                )
                .item(
                    "status_code",
                    AttributeValue::N(record.status_code.to_string()),
                )
                .item(
                    "created_at_ms",
                    AttributeValue::N(record.created_at_ms.to_string()),
                )
                .send()
                .await
                .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
            Ok(())
        }
    }
}

#[cfg(feature = "dynamodb")]
pub use dynamodb_impl::DynamoDbIdempotencyStore;
