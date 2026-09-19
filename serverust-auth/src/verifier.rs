//! O que o [`crate::AuthLayer`] precisa de um verificador de token.

use crate::claims::Claims;
use crate::error::AuthError;

/// Verifica um token e devolve as claims tipadas.
///
/// Existe para que o layer não seja casado com uma origem de chave. Há duas
/// implementações no crate — [`crate::JwtAuth`] (chave estática) e
/// [`crate::JwksAuth`] (chaves do JWKS do emissor) — e nada impede uma
/// terceira, escrita pelo usuário.
///
/// # Por que é síncrono
///
/// A verificação roda em **toda** requisição. Deixá-la assíncrona obrigaria o
/// layer a produzir um future próprio no caminho quente, em vez de repassar o
/// do inner service sem custo. Todo o I/O — descoberta de OIDC, busca do JWKS
/// — acontece na construção do verificador, que é onde a ADR 0009 quer que
/// aconteça: na fase de init da Lambda, onde há burst de CPU, e não na
/// primeira requisição de cada container frio.
pub trait Verifier: Send + Sync + 'static {
    /// Verifica assinatura e claims registradas.
    ///
    /// Recebe `&self`: o verificador não muda de estado ao verificar.
    fn verify<C: Claims>(&self, token: &str) -> Result<C, AuthError>;
}
