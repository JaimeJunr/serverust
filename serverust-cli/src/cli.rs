use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// CLI declarativa do serverust (paridade conceitual com @nestjs/cli).
#[derive(Parser, Debug)]
#[command(name = "serverust", version, about = "serverust framework CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Cria um novo projeto serverust.
    New {
        /// Nome do projeto (vira o nome do diretório e do crate).
        name: String,
    },
    /// Gera scaffolding de um recurso/módulo/componente.
    Generate {
        /// Tipo do artefato.
        kind: GenerateKind,
        /// Nome em snake/kebab case (`users`, `auth`, etc.).
        name: String,
        /// No tipo `module`, gera esqueleto CRUD (DTO + testes de módulo).
        #[arg(long)]
        crud: bool,
    },
    /// Sobe servidor local com hot-reload via `cargo watch`.
    Dev,
    /// Executa `cargo build`.
    Build {
        /// Builda em release mode.
        #[arg(long)]
        release: bool,
    },
    /// Faz deploy para alvos suportados.
    Deploy {
        #[command(subcommand)]
        target: DeployTarget,
    },
    /// Imprime informações sobre a CLI e o ambiente.
    Info,
    /// Valida setup local e requisitos mínimos de telemetria/configuração.
    Doctor,
    /// Operações de OpenAPI: export da spec e geração de client SDK.
    Openapi {
        #[command(subcommand)]
        command: OpenapiCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum OpenapiCommand {
    /// Exporta o spec OpenAPI gerado pelo binário do projeto.
    Export {
        /// Caminho do arquivo de saída (ex.: openapi.json).
        #[arg(long)]
        out: PathBuf,
    },
    /// Gera client SDK a partir do OpenAPI (TS/Kotlin).
    Client {
        /// Linguagem alvo do SDK.
        #[arg(long, value_enum)]
        lang: OpenapiClientLang,
        /// Arquivo OpenAPI de entrada.
        #[arg(long, default_value = "openapi.json")]
        input: PathBuf,
        /// Diretório de saída do client.
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum DeployTarget {
    /// Deploy para AWS Lambda via `cargo lambda deploy`.
    Lambda {
        /// Arquitetura do binário Lambda.
        #[arg(long, value_enum, default_value_t = Arch::Arm64)]
        arch: Arch,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Arch {
    #[value(name = "arm64")]
    Arm64,
    #[value(name = "x86_64", alias = "x86-64")]
    X86_64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum GenerateKind {
    Resource,
    Module,
    Controller,
    Service,
    Pipe,
    Guard,
    Interceptor,
    Filter,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum OpenapiClientLang {
    #[value(name = "ts", alias = "typescript")]
    Ts,
    #[value(name = "kotlin")]
    Kotlin,
}
