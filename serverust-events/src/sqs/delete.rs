//! Abstração de `DeleteMessageBatch` para standalone workers (US-003).
//!
//! Em Lambda ESM, o auto-delete é gerenciado pelo runtime via `batchItemFailures`:
//! mensagens com sucesso são deletadas automaticamente; mensagens com erro ficam
//! visíveis para retry. Não há chamadas manuais ao AWS SDK nesse modo.
//!
//! Em standalone workers (US-010), o código precisa chamar
//! `DeleteMessageBatch` explicitamente após processar com sucesso. Este módulo
//! fornece:
//!
//! - [`DeleteClient`]: trait abstraindo a chamada de rede — mockável em testes.
//! - [`DeleteEntry`]: uma entrada de delete (id único + receipt_handle).
//! - [`DeleteResult`]: IDs que falharam no batch.
//! - [`DeleteManager`]: orquestra o delete com retry exponencial e log de warning.
//!
//! O `DeleteManager` é "fire-and-forget" por design: retenta até `max_retries`
//! vezes, loga warning em stderr a cada falha, e descarta silenciosamente após
//! esgotar as tentativas (mensagens voltam para a fila pelo visibility timeout).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

/// Entrada para `DeleteMessageBatch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteEntry {
    /// ID único dentro do batch (pode ser o message_id).
    pub id: String,
    /// Receipt handle da mensagem a deletar.
    pub receipt_handle: String,
}

impl DeleteEntry {
    pub fn new(id: impl Into<String>, receipt_handle: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            receipt_handle: receipt_handle.into(),
        }
    }
}

/// Resultado de um `DeleteMessageBatch`.
pub struct DeleteResult {
    /// IDs das entradas que falharam (devem ser retentadas).
    pub failed: Vec<String>,
}

/// Trait que abstrai a chamada de rede `DeleteMessageBatch`.
///
/// Implemente sobre o `aws-sdk-sqs` client em código de produção. Em testes,
/// use um mock que controla quais IDs "falham".
#[async_trait]
pub trait DeleteClient: Send + Sync {
    async fn delete_batch(
        &self,
        queue_url: &str,
        entries: Vec<DeleteEntry>,
    ) -> Result<DeleteResult, String>;
}

/// Orquestra `DeleteMessageBatch` com retry exponencial.
///
/// - Retenta apenas as entradas que falharam (partial batch failure).
/// - Em erro de rede, retenta todo o batch pendente.
/// - Loga warning em stderr em cada falha (métrica EMF adicionada em US-012).
/// - Encerra silenciosamente após `max_retries` tentativas; mensagens voltam
///   para a fila pelo visibility timeout, evitando perda de dados.
pub struct DeleteManager<C> {
    client: Arc<C>,
    max_retries: u32,
    base_backoff: Duration,
}

impl<C: DeleteClient> DeleteManager<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            max_retries: 3,
            base_backoff: Duration::from_millis(100),
        }
    }

    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Zera o backoff — útil em testes para não esperar delays reais.
    pub fn with_zero_backoff(mut self) -> Self {
        self.base_backoff = Duration::ZERO;
        self
    }

    /// Deleta as entradas de sucesso confirmado com retry exponencial.
    ///
    /// Não bloqueia o caller em caso de falha persistente — apenas loga e retorna.
    pub async fn delete_successful(&self, queue_url: &str, mut pending: Vec<DeleteEntry>) {
        if pending.is_empty() {
            return;
        }

        for attempt in 1..=self.max_retries {
            match self.client.delete_batch(queue_url, pending.clone()).await {
                Ok(result) if result.failed.is_empty() => return,
                Ok(result) => {
                    pending.retain(|e| result.failed.contains(&e.id));
                    eprintln!(
                        "[serverust-events] sqs delete partial failure (attempt {attempt}/{}): \
                         {} entrada(s) nao deletadas, retentando",
                        self.max_retries,
                        pending.len()
                    );
                }
                Err(e) => {
                    eprintln!(
                        "[serverust-events] sqs delete batch error (attempt {attempt}/{}): {e}",
                        self.max_retries
                    );
                }
            }

            if attempt < self.max_retries && !self.base_backoff.is_zero() {
                let delay = self.base_backoff * 2u32.pow(attempt - 1);
                tokio::time::sleep(delay).await;
            }
        }

        if !pending.is_empty() {
            eprintln!(
                "[serverust-events] sqs delete esgotou {} tentativas para {} entrada(s); \
                 mensagens voltarao para a fila pelo visibility timeout",
                self.max_retries,
                pending.len()
            );
        }
    }
}
