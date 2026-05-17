use std::path::Path;
use std::process::Command;

use crate::cli::{Arch, OpenapiClientLang};

/// `cargo build [--release]`.
pub fn build_cargo_command(release: bool) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if release {
        cmd.arg("--release");
    }
    cmd
}

/// `cargo watch -q -x run` — hot-reload local (quiet: sem prefixo "[Running...]").
pub fn dev_cargo_command() -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("watch").arg("-q").arg("-x").arg("run");
    cmd
}

/// `cargo lambda deploy --<arch>` — deploy Lambda.
pub fn deploy_lambda_cargo_command(arch: Arch) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("lambda").arg("deploy");
    match arch {
        Arch::Arm64 => cmd.arg("--arm64"),
        Arch::X86_64 => cmd.arg("--x86-64"),
    };
    cmd
}

/// `cargo run --quiet -- --serverust-emit-openapi <out>` — extrai o spec OpenAPI
/// do binário do projeto sem subir o servidor. O runtime `serverust_lambda::run`
/// (ou um handler equivalente no `main`) detecta a flag, escreve o spec e sai.
pub fn openapi_export_command(out: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--quiet")
        .arg("--")
        .arg("--serverust-emit-openapi")
        .arg(out);
    cmd
}

/// `cargo run --quiet -- --serverust-emit-asyncapi <out>` — extrai o spec
/// AsyncAPI 3.0 do binário do projeto. O `main` do projeto detecta a flag,
/// monta o `AsyncApiBuilder` com os tipos de evento da aplicação, grava o
/// YAML em `<out>` e sai sem subir consumer/producer.
pub fn asyncapi_export_command(out: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--quiet")
        .arg("--")
        .arg("--serverust-emit-asyncapi")
        .arg(out);
    cmd
}

/// `openapi-generator-cli generate -g <lang> -i <input> -o <out>`.
pub fn openapi_client_command(lang: OpenapiClientLang, input: &Path, out: &Path) -> Command {
    let mut cmd = Command::new("openapi-generator-cli");
    let generator = match lang {
        OpenapiClientLang::Ts => "typescript-fetch",
        OpenapiClientLang::Kotlin => "kotlin",
    };
    cmd.arg("generate")
        .arg("-g")
        .arg(generator)
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(out);
    cmd
}

/// Texto exibido pelo subcomando `info`.
///
/// As features listadas são as do framework serverust-telemetry disponíveis
/// para projetos gerados (não as do próprio binário CLI).
pub fn info_text() -> String {
    let framework_features = ["otel (opt-in)", "dynamodb (opt-in)"];
    format!(
        "serverust-cli {version}\narch: {arch}\nframework features: {features}",
        version = env!("CARGO_PKG_VERSION"),
        arch = std::env::consts::ARCH,
        features = framework_features.join(", "),
    )
}

/// Diagnóstico rápido do ambiente local para desenvolvimento serverust.
pub fn doctor_report(base: &Path) -> String {
    let mut lines = Vec::new();
    let cfg = base.join("serverust.toml");
    let cargo = base.join("Cargo.toml");

    lines.push("serverust doctor".to_string());
    lines.push(format!(
        "cwd: {}",
        base.to_str().unwrap_or("<invalid utf8 path>")
    ));

    if cargo.exists() {
        lines.push("✓ Cargo.toml encontrado".to_string());
    } else {
        lines.push("✗ Cargo.toml ausente".to_string());
    }

    if cfg.exists() {
        lines.push("✓ serverust.toml encontrado".to_string());
        match std::fs::read_to_string(&cfg) {
            Ok(text) => {
                for section in [
                    "[default.telemetry]",
                    "[default.openapi]",
                    "[default.server]",
                ] {
                    if text.contains(section) {
                        lines.push(format!("✓ seção {section}"));
                    } else {
                        lines.push(format!("✗ seção {section} ausente"));
                    }
                }
            }
            Err(_) => lines.push("✗ falha ao ler serverust.toml".to_string()),
        }
    } else {
        lines.push("✗ serverust.toml ausente".to_string());
    }

    lines.push(if tool_available("cargo-watch") {
        "✓ cargo-watch instalado".to_string()
    } else {
        "✗ cargo-watch ausente (instale: cargo install cargo-watch)".to_string()
    });
    lines.push(if tool_available("cargo-lambda") {
        "✓ cargo-lambda instalado".to_string()
    } else {
        "✗ cargo-lambda ausente (instale: cargo install cargo-lambda)".to_string()
    });

    lines.push(if std::env::var("RUST_LOG").is_ok() {
        "✓ RUST_LOG configurado".to_string()
    } else {
        "⚠ RUST_LOG não configurado (default: info)".to_string()
    });

    lines.join("\n")
}

fn tool_available(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
