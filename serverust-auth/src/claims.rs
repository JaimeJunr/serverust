//! Claims de um token e os fatos de autorização que elas expõem.

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// Visão de autorização sobre um conjunto de claims, apagada de tipo.
///
/// Os guards de autorização consomem esta trait em vez do tipo concreto de
/// claims — é o que permite que um guard genérico funcione com qualquer
/// formato de token.
///
/// `has_scope` e `has_role` têm default `false`: um tipo de claims que só
/// carrega identidade nega toda autorização em vez de concedê-la por omissão.
pub trait AuthzFacts: Send + Sync + 'static {
    /// Identificador do principal — tipicamente a claim `sub`.
    fn subject(&self) -> &str;

    /// Se o principal possui o escopo informado.
    fn has_scope(&self, _scope: &str) -> bool {
        false
    }

    /// Se o principal possui o papel informado.
    fn has_role(&self, _role: &str) -> bool {
        false
    }
}

/// Claims desserializáveis de um JWT que expõem fatos de autorização.
///
/// Não é implementada à mão: há impl cega para todo tipo que seja
/// `DeserializeOwned` + [`AuthzFacts`].
pub trait Claims: AuthzFacts + DeserializeOwned {}

impl<T> Claims for T where T: AuthzFacts + DeserializeOwned {}

/// Claims cobrindo os formatos mais comuns de OAuth 2.0 e OIDC.
///
/// Serve para começar sem escrever um tipo próprio. Formatos fora do que está
/// coberto aqui pedem um tipo do seu domínio implementando [`AuthzFacts`] —
/// é o caminho esperado, não um plano B.
///
/// Escopos são lidos de `scope` (string separada por espaço, o formato da
/// RFC 6749) e de `scp` (array, usado por alguns emissores). Papéis são lidos
/// de `roles` e de `groups`.
#[derive(Debug, Clone, Deserialize)]
pub struct StandardClaims {
    /// Identificador do principal.
    pub sub: String,

    /// Emissor do token.
    #[serde(default)]
    pub iss: Option<String>,

    /// Expiração, em segundos desde a época Unix.
    #[serde(default)]
    pub exp: Option<u64>,

    /// Escopos no formato da RFC 6749: separados por espaço.
    #[serde(default)]
    pub scope: Option<String>,

    /// Escopos em array, formato de alguns emissores.
    #[serde(default)]
    pub scp: Vec<String>,

    /// Papéis do principal.
    #[serde(default)]
    pub roles: Vec<String>,

    /// Grupos do principal, tratados como papéis.
    #[serde(default)]
    pub groups: Vec<String>,
}

impl AuthzFacts for StandardClaims {
    fn subject(&self) -> &str {
        &self.sub
    }

    fn has_scope(&self, scope: &str) -> bool {
        let in_space_separated = self
            .scope
            .as_deref()
            .is_some_and(|raw| raw.split_whitespace().any(|s| s == scope));

        in_space_separated || self.scp.iter().any(|s| s == scope)
    }

    fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role) || self.groups.iter().any(|g| g == role)
    }
}
