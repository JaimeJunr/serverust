//! Política de retentativa para inscrições do [`crate::router::EventRouter`].
//!
//! Esta US-3 introduz **apenas o tipo** público `RetryPolicy` para que o
//! builder seja capaz de aceitar `with_retry(...)` na API fluente. A lógica
//! de aplicação (loop de retentativas, jitter, integração com DLQ) é
//! escopo de US-5 — manter este módulo livre de dependências runtime
//! preserva o invariante de cold start do `serverust-core`.

use std::time::Duration;

/// Política de retentativa aplicada a uma inscrição do `EventRouter`.
///
/// Variantes seguem `PRD §RF-3`:
/// - [`RetryPolicy::Immediate`] — `max_attempts` tentativas sem backoff;
/// - [`RetryPolicy::Exponential`] — backoff exponencial a partir de `base_delay`.
///
/// O construtor sem chave de variante (`RetryPolicy::immediate`,
/// `RetryPolicy::exponential`) é o caminho público recomendado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryPolicy {
    /// Retentar imediatamente até `max_attempts` vezes.
    Immediate {
        /// Número máximo de tentativas (1 = sem retry).
        max_attempts: u32,
    },
    /// Backoff exponencial: `base_delay`, `2*base_delay`, `4*base_delay`, ...
    Exponential {
        /// Número máximo de tentativas (1 = sem retry).
        max_attempts: u32,
        /// Atraso base entre tentativas; dobrado a cada falha.
        base_delay: Duration,
    },
}

impl RetryPolicy {
    /// Cria uma política de retentativas imediatas.
    pub fn immediate(max_attempts: u32) -> Self {
        Self::Immediate { max_attempts }
    }

    /// Cria uma política de backoff exponencial.
    pub fn exponential(max_attempts: u32, base_delay: Duration) -> Self {
        Self::Exponential {
            max_attempts,
            base_delay,
        }
    }

    /// Retorna o número máximo de tentativas configurado.
    pub fn max_attempts(&self) -> u32 {
        match self {
            Self::Immediate { max_attempts } | Self::Exponential { max_attempts, .. } => {
                *max_attempts
            }
        }
    }
}
