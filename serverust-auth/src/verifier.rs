//! O que o [`crate::AuthLayer`] precisa de um verificador de token.

use std::future::Future;

use crate::claims::Claims;
use crate::error::AuthError;

/// Verifica um token e devolve as claims tipadas.
///
/// Existe para que o layer não seja casado com uma origem de chave. Há duas
/// implementações no crate — [`crate::JwtAuth`] (chave estática) e
/// [`crate::JwksAuth`] (chaves do JWKS do emissor) — e nada impede uma
/// terceira, escrita pelo usuário.
///
/// # Duas tentativas, e por que a primeira é síncrona
///
/// [`Verifier::verify`] roda em **toda** requisição e não faz I/O. É o que
/// permite ao layer repassar o future do inner service sem alocar nada no
/// caminho quente.
///
/// [`Verifier::revalidate`] só é chamada quando a primeira falhou **e**
/// [`Verifier::can_revalidate`] disse que vale tentar de novo. É o caminho
/// lento, assíncrono, e existe por um caso só: o emissor rotacionou as chaves
/// depois de este processo ter carregado o JWKS. O default não revalida nada,
/// então quem usa chave estática não paga por isto.
pub trait Verifier: Send + Sync + 'static {
    /// Verifica assinatura e claims registradas, sem I/O.
    ///
    /// Recebe `&self`: o verificador não muda de estado ao verificar.
    fn verify<C: Claims>(&self, token: &str) -> Result<C, AuthError>;

    /// Se vale tentar de novo depois deste erro, buscando material novo.
    ///
    /// Consultada antes de [`Verifier::revalidate`] para que o caminho lento
    /// nem seja montado na esmagadora maioria das falhas — token ausente,
    /// expirado, assinatura inválida. Nenhuma dessas melhora com chave nova.
    ///
    /// Default `false`: quem não implementa não muda de comportamento.
    fn can_revalidate(&self, _err: &AuthError) -> bool {
        false
    }

    /// Segunda tentativa, podendo fazer I/O para obter material de chave novo.
    ///
    /// Só é chamada quando [`Verifier::can_revalidate`] devolveu `true`. O
    /// default devolve o erro original sem tocar em nada, o que mantém o
    /// comportamento de quem não implementa.
    ///
    /// Implementações **precisam** limitar a frequência do I/O: este caminho é
    /// alcançável por requisição não autenticada, e sem limite vira um
    /// amplificador contra o próprio emissor.
    fn revalidate<C: Claims>(
        &self,
        _token: &str,
        err: AuthError,
    ) -> impl Future<Output = Result<C, AuthError>> + Send {
        async move { Err(err) }
    }
}
