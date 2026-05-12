use serverust_core::config::{
    LambdaConfig, OpenApiConfig, ServerustConfig, ServerConfig, TelemetryConfig,
};
use std::fs;
use std::sync::Mutex;
use tempfile::TempDir;

// Garante serialização dos testes que mutam variáveis de ambiente
static ENV_MUTEX: Mutex<()> = Mutex::new(());

const BASE_TOML: &str = r#"
[default.server]
host = "127.0.0.1"
port = 3000

[default.lambda]
memory_size = 128
timeout_seconds = 30

[default.telemetry]
log_level = "info"
format = "json"

[default.openapi]
title = "Test API"
version = "1.0.0"
docs_path = "/docs"
redoc_path = "/redoc"

[dev.server]
port = 3001

[prod.server]
host = "0.0.0.0"
port = 8080

[staging.server]
host = "0.0.0.0"
port = 4000
"#;

#[test]
fn test_load_from_file_default_profile() {
    // mutex garante que testes env não contaminam este teste em paralelo
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("serverust.toml");
    fs::write(&path, BASE_TOML).unwrap();

    let cfg = ServerustConfig::load_from(path.to_str().unwrap()).unwrap();
    assert_eq!(cfg.server.host, "127.0.0.1");
    assert_eq!(cfg.server.port, 3000);
    assert_eq!(cfg.lambda.memory_size, 128);
    assert_eq!(cfg.lambda.timeout_seconds, 30);
    assert_eq!(cfg.telemetry.log_level, "info");
    assert_eq!(cfg.telemetry.format, "json");
    assert_eq!(cfg.openapi.title, "Test API");
    assert_eq!(cfg.openapi.version, "1.0.0");
    assert_eq!(cfg.openapi.docs_path, "/docs");
    assert_eq!(cfg.openapi.redoc_path, "/redoc");
}

#[test]
fn test_profile_dev_overrides_server_port() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("serverust.toml");
    fs::write(&path, BASE_TOML).unwrap();

    let cfg = ServerustConfig::load_from_for_profile(path.to_str().unwrap(), "dev").unwrap();
    // porta sobrescrita pelo perfil dev
    assert_eq!(cfg.server.port, 3001);
    // host herdado do default
    assert_eq!(cfg.server.host, "127.0.0.1");
    // demais seções herdadas do default
    assert_eq!(cfg.lambda.memory_size, 128);
    assert_eq!(cfg.openapi.title, "Test API");
}

#[test]
fn test_profile_prod() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("serverust.toml");
    fs::write(&path, BASE_TOML).unwrap();

    let cfg = ServerustConfig::load_from_for_profile(path.to_str().unwrap(), "prod").unwrap();
    assert_eq!(cfg.server.host, "0.0.0.0");
    assert_eq!(cfg.server.port, 8080);
    assert_eq!(cfg.telemetry.log_level, "info"); // herdado
}

#[test]
fn test_profile_staging() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("serverust.toml");
    fs::write(&path, BASE_TOML).unwrap();

    let cfg = ServerustConfig::load_from_for_profile(path.to_str().unwrap(), "staging").unwrap();
    assert_eq!(cfg.server.host, "0.0.0.0");
    assert_eq!(cfg.server.port, 4000);
}

#[test]
fn test_env_override_server_port() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("serverust.toml");
    fs::write(&path, BASE_TOML).unwrap();

    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { std::env::set_var("SERVERUST_SERVER__PORT", "9090") };
    let result = ServerustConfig::load_from(path.to_str().unwrap());
    unsafe { std::env::remove_var("SERVERUST_SERVER__PORT") };

    let cfg = result.unwrap();
    assert_eq!(cfg.server.port, 9090);
    assert_eq!(cfg.server.host, "127.0.0.1"); // não alterado
}

#[test]
fn test_env_override_telemetry_log_level() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("serverust.toml");
    fs::write(&path, BASE_TOML).unwrap();

    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { std::env::set_var("SERVERUST_TELEMETRY__LOG_LEVEL", "debug") };
    let result = ServerustConfig::load_from(path.to_str().unwrap());
    unsafe { std::env::remove_var("SERVERUST_TELEMETRY__LOG_LEVEL") };

    let cfg = result.unwrap();
    assert_eq!(cfg.telemetry.log_level, "debug");
}

#[test]
fn test_default_values() {
    let cfg = ServerustConfig::default();
    assert_eq!(cfg.server.host, "127.0.0.1");
    assert_eq!(cfg.server.port, 3000);
    assert_eq!(cfg.lambda.memory_size, 128);
    assert_eq!(cfg.lambda.timeout_seconds, 30);
    assert_eq!(cfg.telemetry.log_level, "info");
    assert_eq!(cfg.telemetry.format, "json");
    assert_eq!(cfg.openapi.title, "serverust");
    assert_eq!(cfg.openapi.version, "0.1.0");
    assert_eq!(cfg.openapi.docs_path, "/docs");
    assert_eq!(cfg.openapi.redoc_path, "/redoc");
}

#[test]
fn test_config_structs_accessible() {
    // Verifica que os tipos são acessíveis publicamente
    let _server = ServerConfig {
        host: "0.0.0.0".to_string(),
        port: 8080,
    };
    let _lambda = LambdaConfig {
        memory_size: 256,
        timeout_seconds: 60,
    };
    let _telemetry = TelemetryConfig {
        log_level: "warn".to_string(),
        format: "text".to_string(),
    };
    let _openapi = OpenApiConfig {
        title: "My API".to_string(),
        version: "2.0.0".to_string(),
        docs_path: "/swagger".to_string(),
        redoc_path: "/redoc".to_string(),
    };
}
