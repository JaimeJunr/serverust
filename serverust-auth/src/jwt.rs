//! Verificador de JWT com chave estática.
//!
//! Cobre o caso em que a chave é conhecida no boot: segredo simétrico vindo de
//! variável de ambiente ou Secrets Manager, ou chave pública embutida no
//! binário. Não faz I/O — a verificação é síncrona e não toca a rede.

use http::HeaderMap;
use http::header::AUTHORIZATION;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

use crate::claims::Claims;
use crate::error::AuthError;

/// Verificador de tokens com chave estática.
///
/// O algoritmo é fixado no construtor e **não** é lido do header do token.
/// Isso fecha a classe de ataque de confusão de algoritmo, em que um token
/// forjado declara `alg: HS256` para que a chave pública RSA do servidor seja
/// usada como segredo HMAC.
///
/// ```no_run
/// use serverust_auth::JwtAuth;
///
/// let auth = JwtAuth::hs256(b"segredo")
///     .issuer("https://idp.exemplo.com/")
///     .audience("minha-api");
/// ```
pub struct JwtAuth {
    key: DecodingKey,
    validation: Validation,
}

impl JwtAuth {
    fn with(key: DecodingKey, alg: Algorithm) -> Self {
        // `Validation::new` exige a claim `exp` e valida expiração — token sem
        // `exp` é rejeitado, e não afrouxamos isso.
        //
        // Atenção ao que esse default NÃO é: o `jsonwebtoken` embute
        // `leeway: 60`, então um token expirado há até 60 s ainda passa. É a
        // tolerância padrão da indústria para desalinhamento de relógio entre
        // emissor e verificador, e mantemos. Quem precisa de janela diferente
        // usa [`JwtAuth::leeway`].
        Self {
            key,
            validation: Validation::new(alg),
        }
    }

    /// Verificador HMAC-SHA256 a partir de um segredo compartilhado.
    pub fn hs256(secret: &[u8]) -> Self {
        Self::with(DecodingKey::from_secret(secret), Algorithm::HS256)
    }

    /// Verificador RSA-SHA256 a partir de uma chave **pública** em PEM.
    pub fn rs256_pem(pem: &[u8]) -> Result<Self, AuthError> {
        let key =
            DecodingKey::from_rsa_pem(pem).map_err(|e| AuthError::InvalidKey(e.to_string()))?;
        Ok(Self::with(key, Algorithm::RS256))
    }

    /// Verificador ECDSA P-256 a partir de uma chave **pública** em PEM.
    pub fn es256_pem(pem: &[u8]) -> Result<Self, AuthError> {
        let key =
            DecodingKey::from_ec_pem(pem).map_err(|e| AuthError::InvalidKey(e.to_string()))?;
        Ok(Self::with(key, Algorithm::ES256))
    }

    /// Exige que a claim `iss` case com o emissor informado.
    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        self.validation.set_issuer(&[issuer.into()]);
        self
    }

    /// Exige que a claim `aud` case com a audiência informada.
    pub fn audience(mut self, audience: impl Into<String>) -> Self {
        self.validation.set_audience(&[audience.into()]);
        self
    }

    /// Tolerância, em segundos, na checagem de `exp` e `nbf`. Compensa
    /// desalinhamento de relógio entre emissor e verificador.
    ///
    /// O default herdado do `jsonwebtoken` é **60 segundos** — não zero. Passe
    /// `0` para exigir expiração estrita, ao custo de 401 intermitente quando
    /// os relógios divergirem.
    pub fn leeway(mut self, seconds: u64) -> Self {
        self.validation.leeway = seconds;
        self
    }

    /// Verifica assinatura e claims registradas, devolvendo o payload tipado.
    pub fn verify<C: Claims>(&self, token: &str) -> Result<C, AuthError> {
        decode::<C>(token, &self.key, &self.validation)
            .map(|data| data.claims)
            .map_err(AuthError::from)
    }
}

// Debug manual: o Debug derivado exporia material de chave em log de panic ou
// em `dbg!`. Nada de `key` nem de `validation` aparece aqui.
impl std::fmt::Debug for JwtAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtAuth").finish_non_exhaustive()
    }
}

/// Extrai o token de `Authorization: Bearer <token>`.
///
/// O nome do esquema é comparado sem diferenciar maiúsculas, como manda a
/// RFC 7235 §2.1.
pub(crate) fn bearer_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    let header = headers.get(AUTHORIZATION).ok_or(AuthError::Missing)?;
    let value = header.to_str().map_err(|_| AuthError::MalformedHeader)?;

    let (scheme, token) = value.split_once(' ').ok_or(AuthError::MalformedHeader)?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(AuthError::MalformedHeader);
    }

    let token = token.trim();
    if token.is_empty() {
        return Err(AuthError::MalformedHeader);
    }

    Ok(token)
}
