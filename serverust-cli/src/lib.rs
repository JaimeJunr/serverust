//! CLI do framework **serverust**, com paridade conceitual ao `@nestjs/cli`.
//!
//! O binário `serverust` provê comandos para scaffolding e workflow de
//! desenvolvimento/deploy:
//!
//! ```text
//! serverust new <name>                       # cria um projeto novo
//! serverust generate <kind> <name>           # scaffolding (resource, module, ...)
//! serverust dev                              # cargo watch -x run
//! serverust build [--release]                # cargo build
//! serverust deploy lambda [--arch arm64|x86_64]
//! serverust info                             # versões e features
//! serverust openapi --out openapi.json       # exporta spec sem subir servidor
//! ```
//!
//! Este crate expõe também a lib (`serverust_cli`) com módulos
//! [`cli`] (definições clap), [`commands`] (construção testável de
//! `std::process::Command`), [`scaffold`] (IO em base dir parametrizada) e
//! [`templates`] (strings de scaffolding). A separação permite testar parse +
//! geração de arquivos em tempdir sem spawn de processos reais.

pub mod cli;
pub mod commands;
pub mod scaffold;
pub mod templates;

use anyhow::Result;

use crate::cli::{Cli, Command, DeployTarget};

/// Executa um comando da CLI já parseado.
///
/// Operações de IO (criação de arquivos, spawn de processos) são executadas
/// aqui; a separação em módulos mantém a lógica testável sem efeitos colaterais.
pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::New { name } => {
            let cwd = std::env::current_dir()?;
            scaffold::new_project(&cwd, &name)?;
            println!("✓ project created at {}/{}", cwd.display(), name);
            Ok(())
        }
        Command::Generate { kind, name } => {
            let cwd = std::env::current_dir()?;
            scaffold::generate(&cwd, kind, &name)?;
            println!("✓ {kind:?} '{name}' generated");
            Ok(())
        }
        Command::Dev => {
            require_cargo_subcommand("watch", "cargo install cargo-watch")?;
            spawn_status(commands::dev_cargo_command(), "dev")
        }
        Command::Build { release } => spawn_status(commands::build_cargo_command(release), "build"),
        Command::Deploy { target } => match target {
            DeployTarget::Lambda { arch } => {
                require_cargo_subcommand(
                    "lambda",
                    "cargo install cargo-lambda  # ou https://www.cargo-lambda.info/guide/installation.html",
                )?;
                spawn_status(commands::deploy_lambda_cargo_command(arch), "deploy lambda")
            }
        },
        Command::Info => {
            println!("{}", commands::info_text());
            Ok(())
        }
        Command::Openapi { out } => spawn_status(commands::openapi_export_command(&out), "openapi"),
    }
}

/// Confirma que `cargo-<subcommand>` está disponível no PATH antes de chamar.
///
/// Em vez de deixar o cargo cuspir o erro padrão (`error: no such command: ...`),
/// emitimos uma mensagem com o comando exato de instalação. Reduz fricção para
/// quem está descobrindo o framework e ainda não conhece o ecossistema.
fn require_cargo_subcommand(subcommand: &str, install_hint: &str) -> Result<()> {
    let binary = format!("cargo-{subcommand}");
    let installed = std::process::Command::new(&binary)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if installed {
        return Ok(());
    }
    anyhow::bail!(
        "`cargo {subcommand}` não está disponível (necessário para este comando).\n\
         Instale com:\n    {install_hint}"
    );
}

fn spawn_status(mut cmd: std::process::Command, label: &str) -> Result<()> {
    let status = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("failed to spawn {label}: {e}"))?;
    if !status.success() {
        anyhow::bail!("{label} failed with status {status}");
    }
    Ok(())
}
