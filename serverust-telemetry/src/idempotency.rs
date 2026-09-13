//! Trait [`IdempotencyStore`] e implementações disponíveis no MVP.
//!
//! Implementações fornecidas:
//! - [`InMemoryIdempotencyStore`] — referência atômica via `Mutex`, útil em
//!   testes, dev local e workloads single-instance.
//! - `DynamoDbIdempotencyStore` (atrás da feature `dynamodb`) usando
//!   `aws-sdk-dynamodb` com `condition_expression` para garantir lock
//!   atômico cross-instância.
//!
//! # Semântica de idempotência (US-007)
//!
//! Além do `get`/`put` legados (idempotência HTTP), a trait expõe um protocolo
//! de **lock InProgress → Completed** com TTL:
//!
//! - [`IdempotencyStore::try_acquire`] tenta criar um registro `InProgress`
//!   para a chave; retorna [`AcquireOutcome::Acquired`] com um [`LockToken`]
//!   (caller pode rodar), [`AcquireOutcome::InProgress`] (outro worker
//!   possui o lock — caller deve recuar) ou
//!   [`AcquireOutcome::AlreadyCompleted`] (skip seguro).
//! - [`IdempotencyStore::complete`] grava o resultado final (estado
//!   `Completed`) com novo TTL, apenas se o token for o da aquisição corrente.
//! - [`IdempotencyStore::release`] remove um lock `InProgress` abandonado
//!   após falha do handler ou de `complete`, apenas se o token for o da
//!   aquisição corrente, permitindo que o SQS redelivery reexecute o
//!   processamento dentro do TTL.
//!
//! Em DynamoDB, isso é implementado com `condition_expression`
//! `attribute_not_exists(pk) OR expires_at_ms < :now` no `try_acquire`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Registro HTTP legado (cache de resposta por chave de idempotência).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub key: String,
    pub response_body: Vec<u8>,
    pub status_code: u16,
    /// Timestamp epoch ms para suportar TTL no storage destino.
    pub created_at_ms: u64,
}

/// Estado do lock de idempotência.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdempotencyState {
    /// Lock tomado mas resultado ainda não confirmado.
    InProgress,
    /// Processamento concluído com sucesso — `try_acquire` retorna `AlreadyCompleted`.
    Completed,
}

/// Token opaco de ownership, único por aquisição bem-sucedida.
///
/// `release` e `complete` só mutam o registro se este token bater com o
/// persistido — um owner cujo TTL expirou não apaga nem completa o lock
/// de quem adquiriu depois.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LockToken(String);

impl LockToken {
    /// Emite um token novo (UUID v4) para uma aquisição.
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Valor persistido no storage (DynamoDB atributo `token`, etc.).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reconstrói o token a partir do valor persistido.
    pub fn from_stored(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// Registro de lock — estado + TTL + token de fencing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyLockRecord {
    pub key: String,
    pub state: IdempotencyState,
    pub created_at_ms: u64,
    /// Epoch ms em que o lock expira. Após esse instante, qualquer
    /// `try_acquire` deve sobrescrever o registro.
    pub expires_at_ms: u64,
    /// Token da aquisição que criou (ou reescreveu) este registro.
    pub token: LockToken,
}

/// Resultado de [`IdempotencyStore::try_acquire`].
#[derive(Debug, Clone)]
pub enum AcquireOutcome {
    /// Lock adquirido (chave nova OU registro anterior expirado).
    Acquired(LockToken),
    /// Outro worker possui o lock InProgress dentro do TTL.
    InProgress,
    /// Já existe um registro Completed dentro do TTL.
    AlreadyCompleted(IdempotencyLockRecord),
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
    /// Retorna o registro HTTP existente (legado) ou `None`.
    async fn get(&self, key: &str) -> Result<Option<IdempotencyRecord>, IdempotencyError>;

    /// Persiste o registro HTTP (legado), sobrescrevendo se existir.
    async fn put(&self, record: IdempotencyRecord) -> Result<(), IdempotencyError>;

    /// Tenta adquirir lock InProgress para a chave. Atômico no storage
    /// (DynamoDB: condition expression; InMemory: Mutex).
    async fn try_acquire(
        &self,
        key: &str,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Result<AcquireOutcome, IdempotencyError>;

    /// Marca a chave como Completed com novo TTL. Caller deve ter chamado
    /// `try_acquire` antes e obtido `Acquired`. No-op de sucesso se `token`
    /// não for o da aquisição corrente (outro worker detém o lock).
    async fn complete(
        &self,
        key: &str,
        token: &LockToken,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Result<(), IdempotencyError>;

    /// Libera um lock `InProgress` abandonado (ex.: handler falhou após
    /// `try_acquire`). Só remove se `token` bater com o registro corrente.
    /// Token divergente, registros `Completed` e chave ausente são no-op
    /// de sucesso — o lock é de outro dono, não é erro nosso.
    async fn release(&self, key: &str, token: &LockToken) -> Result<(), IdempotencyError>;
}

/// Implementação in-memory thread-safe — referência para testes e dev.
#[derive(Debug, Default)]
pub struct InMemoryIdempotencyStore {
    http: Mutex<HashMap<String, IdempotencyRecord>>,
    locks: Mutex<HashMap<String, IdempotencyLockRecord>>,
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
            .http
            .lock()
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
        Ok(guard.get(key).cloned())
    }

    async fn put(&self, record: IdempotencyRecord) -> Result<(), IdempotencyError> {
        let mut guard = self
            .http
            .lock()
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
        guard.insert(record.key.clone(), record);
        Ok(())
    }

    async fn try_acquire(
        &self,
        key: &str,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Result<AcquireOutcome, IdempotencyError> {
        let mut guard = self
            .locks
            .lock()
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
        if let Some(existing) = guard.get(key) {
            if existing.expires_at_ms > now_ms {
                return Ok(match existing.state {
                    IdempotencyState::Completed => {
                        AcquireOutcome::AlreadyCompleted(existing.clone())
                    }
                    IdempotencyState::InProgress => AcquireOutcome::InProgress,
                });
            }
        }
        let token = LockToken::generate();
        guard.insert(
            key.to_string(),
            IdempotencyLockRecord {
                key: key.to_string(),
                state: IdempotencyState::InProgress,
                created_at_ms: now_ms,
                expires_at_ms: now_ms.saturating_add(ttl_ms),
                token: token.clone(),
            },
        );
        Ok(AcquireOutcome::Acquired(token))
    }

    async fn complete(
        &self,
        key: &str,
        token: &LockToken,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Result<(), IdempotencyError> {
        let mut guard = self
            .locks
            .lock()
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
        match guard.get(key) {
            Some(existing)
                if existing.state == IdempotencyState::InProgress && existing.token == *token =>
            {
                guard.insert(
                    key.to_string(),
                    IdempotencyLockRecord {
                        key: key.to_string(),
                        state: IdempotencyState::Completed,
                        created_at_ms: now_ms,
                        expires_at_ms: now_ms.saturating_add(ttl_ms),
                        token: token.clone(),
                    },
                );
            }
            _ => {}
        }
        Ok(())
    }

    async fn release(&self, key: &str, token: &LockToken) -> Result<(), IdempotencyError> {
        let mut guard = self
            .locks
            .lock()
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
        match guard.get(key) {
            Some(existing)
                if existing.state == IdempotencyState::InProgress && existing.token == *token =>
            {
                guard.remove(key);
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(feature = "dynamodb")]
mod dynamodb_impl {
    use super::{
        AcquireOutcome, IdempotencyError, IdempotencyLockRecord, IdempotencyRecord,
        IdempotencyState, IdempotencyStore, LockToken,
    };
    use async_trait::async_trait;
    use aws_sdk_dynamodb::Client;
    use aws_sdk_dynamodb::error::SdkError;
    use aws_sdk_dynamodb::operation::put_item::PutItemError;
    use aws_sdk_dynamodb::primitives::Blob;
    use aws_sdk_dynamodb::types::AttributeValue;

    /// Implementação DynamoDB. O schema esperado:
    /// - `pk` (S): chave de idempotência.
    /// - `state` (S): `"InProgress"` ou `"Completed"`.
    /// - `token` (S): token de fencing da aquisição corrente.
    /// - `created_at_ms` (N), `expires_at_ms` (N).
    /// - `response_body` (B), `status_code` (N) — apenas para registros HTTP.
    ///
    /// O TTL pode ser configurado no atributo `expires_at_ms` (em segundos é
    /// recomendado pela AWS; aqui usamos ms para granularidade — usuários
    /// devem usar um campo separado em segundos se quiserem o TTL nativo do
    /// DynamoDB).
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

        async fn try_acquire(
            &self,
            key: &str,
            now_ms: u64,
            ttl_ms: u64,
        ) -> Result<AcquireOutcome, IdempotencyError> {
            let expires_at_ms = now_ms.saturating_add(ttl_ms);
            let token = LockToken::generate();
            // put-if-absent: condicional `attribute_not_exists(pk) OR expires_at_ms < :now`.
            let result = self
                .client
                .put_item()
                .table_name(&self.table_name)
                .item("pk", AttributeValue::S(key.to_string()))
                .item("state", AttributeValue::S("InProgress".to_string()))
                .item("token", AttributeValue::S(token.as_str().to_string()))
                .item("created_at_ms", AttributeValue::N(now_ms.to_string()))
                .item(
                    "expires_at_ms",
                    AttributeValue::N(expires_at_ms.to_string()),
                )
                .condition_expression("attribute_not_exists(pk) OR expires_at_ms < :now")
                .expression_attribute_values(":now", AttributeValue::N(now_ms.to_string()))
                .send()
                .await;

            match result {
                Ok(_) => Ok(AcquireOutcome::Acquired(token)),
                Err(SdkError::ServiceError(svc))
                    if matches!(svc.err(), PutItemError::ConditionalCheckFailedException(_)) =>
                {
                    // Já existe registro dentro do TTL. Carregar para decidir.
                    let existing = self
                        .client
                        .get_item()
                        .table_name(&self.table_name)
                        .key("pk", AttributeValue::S(key.to_string()))
                        .send()
                        .await
                        .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
                    let item = existing.item.ok_or_else(|| {
                        IdempotencyError::Storage(
                            "conditional failed mas item ausente no get".into(),
                        )
                    })?;
                    let state = item
                        .get("state")
                        .and_then(|v| v.as_s().ok())
                        .map(|s| s.as_str())
                        .unwrap_or("InProgress");
                    let created_at_ms = item
                        .get("created_at_ms")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|n| n.parse::<u64>().ok())
                        .unwrap_or(0);
                    let expires_at_ms = item
                        .get("expires_at_ms")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|n| n.parse::<u64>().ok())
                        .unwrap_or(0);
                    let token = item
                        .get("token")
                        .and_then(|v| v.as_s().ok())
                        .map(|s| LockToken::from_stored(s.clone()))
                        .unwrap_or_else(|| LockToken::from_stored(String::new()));
                    let record = IdempotencyLockRecord {
                        key: key.to_string(),
                        state: if state == "Completed" {
                            IdempotencyState::Completed
                        } else {
                            IdempotencyState::InProgress
                        },
                        created_at_ms,
                        expires_at_ms,
                        token,
                    };
                    Ok(match record.state {
                        IdempotencyState::Completed => AcquireOutcome::AlreadyCompleted(record),
                        IdempotencyState::InProgress => AcquireOutcome::InProgress,
                    })
                }
                Err(e) => Err(IdempotencyError::Storage(e.to_string())),
            }
        }

        async fn complete(
            &self,
            key: &str,
            token: &LockToken,
            now_ms: u64,
            ttl_ms: u64,
        ) -> Result<(), IdempotencyError> {
            let expires_at_ms = now_ms.saturating_add(ttl_ms);
            let result = self
                .client
                .put_item()
                .table_name(&self.table_name)
                .item("pk", AttributeValue::S(key.to_string()))
                .item("state", AttributeValue::S("Completed".to_string()))
                .item("token", AttributeValue::S(token.as_str().to_string()))
                .item("created_at_ms", AttributeValue::N(now_ms.to_string()))
                .item(
                    "expires_at_ms",
                    AttributeValue::N(expires_at_ms.to_string()),
                )
                .condition_expression("#state = :in_progress AND #token = :token")
                .expression_attribute_names("#state", "state")
                .expression_attribute_names("#token", "token")
                .expression_attribute_values(
                    ":in_progress",
                    AttributeValue::S("InProgress".to_string()),
                )
                .expression_attribute_values(
                    ":token",
                    AttributeValue::S(token.as_str().to_string()),
                )
                .send()
                .await;

            match result {
                Ok(_) => Ok(()),
                Err(SdkError::ServiceError(svc))
                    if matches!(svc.err(), PutItemError::ConditionalCheckFailedException(_)) =>
                {
                    Ok(())
                }
                Err(e) => Err(IdempotencyError::Storage(e.to_string())),
            }
        }

        async fn release(&self, key: &str, token: &LockToken) -> Result<(), IdempotencyError> {
            let result = self
                .client
                .delete_item()
                .table_name(&self.table_name)
                .key("pk", AttributeValue::S(key.to_string()))
                .condition_expression("#state = :in_progress AND #token = :token")
                .expression_attribute_names("#state", "state")
                .expression_attribute_names("#token", "token")
                .expression_attribute_values(
                    ":in_progress",
                    AttributeValue::S("InProgress".to_string()),
                )
                .expression_attribute_values(
                    ":token",
                    AttributeValue::S(token.as_str().to_string()),
                )
                .send()
                .await;

            match result {
                Ok(_) => Ok(()),
                Err(SdkError::ServiceError(svc))
                    if matches!(
                        svc.err(),
                        aws_sdk_dynamodb::operation::delete_item::DeleteItemError::ConditionalCheckFailedException(_)
                    ) =>
                {
                    Ok(())
                }
                Err(e) => Err(IdempotencyError::Storage(e.to_string())),
            }
        }
    }
}

#[cfg(feature = "dynamodb")]
pub use dynamodb_impl::DynamoDbIdempotencyStore;
