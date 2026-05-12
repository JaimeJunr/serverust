//! Carregamento de configuração via figment (serverust.toml + env vars).
// figment::Error é grande por design (contém contexto de diagnóstico rico);
// suprimir o lint é a escolha idiomática quando o tipo de erro é da API pública.
#![allow(clippy::result_large_err)]

use figment::providers::{Env, Format, Serialized, Toml};
use figment::{Figment, Profile};
use serde::{Deserialize, Serialize};

/// Configuração do servidor HTTP local.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
        }
    }
}

/// Configuração do runtime AWS Lambda.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LambdaConfig {
    pub memory_size: u32,
    pub timeout_seconds: u32,
}

impl Default for LambdaConfig {
    fn default() -> Self {
        Self {
            memory_size: 128,
            timeout_seconds: 30,
        }
    }
}

/// Configuração de telemetria (logger + tracing).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryConfig {
    pub log_level: String,
    pub format: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            format: "json".to_string(),
        }
    }
}

/// Configuração de documentação OpenAPI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenApiConfig {
    pub title: String,
    pub version: String,
    pub docs_path: String,
    pub redoc_path: String,
}

impl Default for OpenApiConfig {
    fn default() -> Self {
        Self {
            title: "serverust".to_string(),
            version: "0.1.0".to_string(),
            docs_path: "/docs".to_string(),
            redoc_path: "/redoc".to_string(),
        }
    }
}

/// Configuração raiz do projeto. Carregada de `serverust.toml` com override por env vars.
///
/// Formato do arquivo (profile-aware via figment):
/// ```toml
/// [default.server]
/// host = "127.0.0.1"
/// port = 3000
///
/// [default.lambda]
/// memory_size = 128
/// timeout_seconds = 30
///
/// [default.telemetry]
/// log_level = "info"
/// format = "json"
///
/// [default.openapi]
/// title = "My API"
/// version = "0.1.0"
/// docs_path = "/docs"
/// redoc_path = "/redoc"
///
/// # Overrides por perfil:
/// [dev.server]
/// port = 3001
/// ```
///
/// Variáveis de ambiente sobrescrevem o arquivo (separador `__` para campos aninhados):
/// - `SERVERUST_SERVER__PORT=8080`
/// - `SERVERUST_TELEMETRY__LOG_LEVEL=debug`
/// - `SERVERUST_PROFILE=prod` para selecionar perfil automaticamente
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ServerustConfig {
    pub server: ServerConfig,
    pub lambda: LambdaConfig,
    pub telemetry: TelemetryConfig,
    pub openapi: OpenApiConfig,
}

impl ServerustConfig {
    /// Carrega de `serverust.toml` na raiz do projeto, perfil `default`.
    /// Override por env vars com prefixo `SERVERUST_` (ex: `SERVERUST_SERVER__PORT=8080`).
    pub fn load() -> Result<Self, figment::Error> {
        Self::load_for_profile(Profile::Default)
    }

    /// Carrega de `serverust.toml` com perfil específico.
    /// Perfil herda dados do `[default]` e os sobrescreve com `[<profile>]`.
    /// O perfil também pode ser definido via `SERVERUST_PROFILE`.
    pub fn load_for_profile(profile: impl Into<Profile>) -> Result<Self, figment::Error> {
        Self::load_from_for_profile("serverust.toml", profile)
    }

    /// Carrega de arquivo específico no perfil `default`. Útil em testes.
    pub fn load_from(path: &str) -> Result<Self, figment::Error> {
        Self::load_from_for_profile(path, Profile::Default)
    }

    /// Carrega de arquivo específico com perfil específico. Útil em testes.
    pub fn load_from_for_profile(
        path: &str,
        profile: impl Into<Profile>,
    ) -> Result<Self, figment::Error> {
        // .nested() faz top-level TOML keys virarem profile names:
        // [default.server] → profile "default", key "server"
        // [dev.server]     → profile "dev", key "server"
        // Env::prefixed usa separador __ para campos aninhados:
        // SERVERUST_SERVER__PORT → server.port
        Figment::new()
            .merge(Serialized::defaults(Self::default()))
            .merge(Toml::file(path).nested())
            .merge(Env::prefixed("SERVERUST_").split("__"))
            .select(profile)
            .extract()
    }
}
