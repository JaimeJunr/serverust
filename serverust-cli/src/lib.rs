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
        Command::Dev => spawn_status(commands::dev_cargo_command(), "dev"),
        Command::Build { release } => spawn_status(commands::build_cargo_command(release), "build"),
        Command::Deploy { target } => match target {
            DeployTarget::Lambda { arch } => {
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

fn spawn_status(mut cmd: std::process::Command, label: &str) -> Result<()> {
    let status = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("failed to spawn {label}: {e}"))?;
    if !status.success() {
        anyhow::bail!("{label} failed with status {status}");
    }
    Ok(())
}
