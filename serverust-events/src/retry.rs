//! Política de retentativa para inscrições do [`crate::router::EventRouter`].
//!
//! `RetryPolicy` é aplicada pelo [`crate::router::EventRouter::attach`]:
//! tentativas imediatas (`Immediate`) ou com backoff exponencial
//! (`Exponential`). Após esgotamento, a mensagem é publicada no DLQ
//! configurado via [`RetryPolicy::dead_letter`] ou
//! [`crate::router::EventRouter::with_dlq`], se houver.

use std::time::Duration;

/// Política de retentativa aplicada a uma inscrição do `EventRouter`.
///
/// Variantes seguem `PRD §RF-3`:
/// - [`RetryPolicy::Immediate`] — `max_attempts` tentativas sem backoff;
/// - [`RetryPolicy::Exponential`] — backoff exponencial a partir de `base_delay`.
///
/// O construtor sem chave de variante (`RetryPolicy::immediate`,
/// `RetryPolicy::exponential`) é o caminho público recomendado.
/// Use [`RetryPolicy::dead_letter`] para configurar DLQ diretamente na política.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryPolicy {
    /// Retentar imediatamente até `max_attempts` vezes.
    Immediate {
        /// Número máximo de tentativas (1 = sem retry).
        max_attempts: u32,
        /// Tópico de dead letter queue opcional.
        dlq: Option<String>,
    },
    /// Backoff exponencial: `base_delay`, `2*base_delay`, `4*base_delay`, ...
    Exponential {
        /// Número máximo de tentativas (1 = sem retry).
        max_attempts: u32,
        /// Atraso base entre tentativas; dobrado a cada falha.
        base_delay: Duration,
        /// Tópico de dead letter queue opcional.
        dlq: Option<String>,
    },
}

impl RetryPolicy {
    /// Cria uma política de retentativas imediatas.
    pub fn immediate(max_attempts: u32) -> Self {
        Self::Immediate {
            max_attempts,
            dlq: None,
        }
    }

    /// Cria uma política de backoff exponencial.
    pub fn exponential(max_attempts: u32, base_delay: Duration) -> Self {
        Self::Exponential {
            max_attempts,
            base_delay,
            dlq: None,
        }
    }

    /// Configura o tópico DLQ: mensagem publicada após esgotamento de tentativas.
    pub fn dead_letter(self, topic: impl Into<String>) -> Self {
        match self {
            Self::Immediate { max_attempts, .. } => Self::Immediate {
                max_attempts,
                dlq: Some(topic.into()),
            },
            Self::Exponential {
                max_attempts,
                base_delay,
                ..
            } => Self::Exponential {
                max_attempts,
                base_delay,
                dlq: Some(topic.into()),
            },
        }
    }

    /// Retorna o número máximo de tentativas configurado.
    pub fn max_attempts(&self) -> u32 {
        match self {
            Self::Immediate { max_attempts, .. } | Self::Exponential { max_attempts, .. } => {
                *max_attempts
            }
        }
    }

    /// Retorna o tópico DLQ configurado nesta política, se houver.
    pub fn dlq_topic(&self) -> Option<&str> {
        match self {
            Self::Immediate { dlq, .. } | Self::Exponential { dlq, .. } => dlq.as_deref(),
        }
    }
}
