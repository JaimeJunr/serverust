//! Erros de autenticação e a resposta HTTP 401 padronizada.

use axum::Json;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use http::header::WWW_AUTHENTICATE;
use serde_json::json;

/// Motivo pelo qual uma requisição não foi autenticada.
///
/// O texto de cada variante é para log; o que vai no corpo da resposta é o
/// código estável de [`AuthError::reason`], para que clientes possam ramificar
/// sem depender de mensagem.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// Nenhum header `Authorization` presente.
    #[error("credencial ausente")]
    Missing,

    /// Header presente mas fora do formato `Authorization: Bearer <token>`.
    #[error("header Authorization malformado")]
    MalformedHeader,

    /// Assinatura válida, mas `exp` no passado.
    #[error("token expirado")]
    Expired,

    /// Claim `iss` diferente do emissor esperado.
    #[error("emissor inválido")]
    InvalidIssuer,

    /// Claim `aud` diferente da audiência esperada.
    #[error("audiência inválida")]
    InvalidAudience,

    /// Assinatura inválida, algoritmo divergente ou payload indesserializável.
    #[error("token inválido")]
    InvalidToken,

    /// Falha ao construir o verificador a partir do material de chave.
    /// Ocorre na inicialização, não no caminho de request.
    #[error("chave inválida: {0}")]
    InvalidKey(String),
}

impl AuthError {
    /// Código estável exposto no corpo da resposta. Faz parte da API pública:
    /// clientes ramificam sobre ele.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Missing => "missing_credentials",
            Self::MalformedHeader => "malformed_authorization_header",
            Self::Expired => "token_expired",
            Self::InvalidIssuer => "invalid_issuer",
            Self::InvalidAudience => "invalid_audience",
            Self::InvalidToken => "invalid_token",
            Self::InvalidKey(_) => "invalid_key",
        }
    }
}

impl From<jsonwebtoken::errors::Error> for AuthError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        use jsonwebtoken::errors::ErrorKind;
        match err.kind() {
            ErrorKind::ExpiredSignature => Self::Expired,
            ErrorKind::InvalidIssuer => Self::InvalidIssuer,
            ErrorKind::InvalidAudience => Self::InvalidAudience,
            _ => Self::InvalidToken,
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        unauthorized(&self)
    }
}

/// Resposta 401 padronizada.
///
/// Inclui `WWW-Authenticate: Bearer` conforme a RFC 6750 §3 — sem esse header
/// um 401 não diz ao cliente qual esquema usar.
pub fn unauthorized(err: &AuthError) -> Response {
    let body = json!({
        "error": "unauthorized",
        "reason": err.reason(),
    });

    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, "Bearer")],
        Json(body),
    )
        .into_response()
}
