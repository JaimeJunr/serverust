use std::path::Path;
use std::process::Command;

use crate::cli::Arch;

/// `cargo build [--release]`.
pub fn build_cargo_command(release: bool) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if release {
        cmd.arg("--release");
    }
    cmd
}

/// `cargo watch -x run` — hot-reload local.
pub fn dev_cargo_command() -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("watch").arg("-x").arg("run");
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
