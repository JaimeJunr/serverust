//! Testes da flag `serverust info --asyncapi` e do builder de comando.

use clap::Parser;
use serverust_cli::cli::{Cli, Command};
use serverust_cli::commands;

#[test]
fn parses_info_without_asyncapi_flag() {
    let cli = Cli::try_parse_from(["serverust", "info"]).expect("parse");
    match cli.command {
        Command::Info { asyncapi, out } => {
            assert!(!asyncapi);
            assert!(out.is_none());
        }
        other => panic!("expected Info, got {other:?}"),
    }
}

#[test]
fn parses_info_with_asyncapi_flag() {
    let cli = Cli::try_parse_from(["serverust", "info", "--asyncapi"]).expect("parse");
    match cli.command {
        Command::Info { asyncapi, out } => {
            assert!(asyncapi);
            assert!(out.is_none());
        }
        other => panic!("expected Info, got {other:?}"),
    }
}

#[test]
fn parses_info_with_asyncapi_and_out_path() {
    let cli = Cli::try_parse_from(["serverust", "info", "--asyncapi", "--out", "spec.yaml"])
        .expect("parse");
    match cli.command {
        Command::Info { asyncapi, out } => {
            assert!(asyncapi);
            assert_eq!(out.as_deref().and_then(|p| p.to_str()), Some("spec.yaml"));
        }
        other => panic!("expected Info, got {other:?}"),
    }
}

#[test]
fn asyncapi_export_command_passes_out_path() {
    let cmd = commands::asyncapi_export_command(std::path::Path::new("asyncapi.yaml"));
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(program, "cargo");
    assert!(args.iter().any(|a| a == "run"));
    assert!(args.iter().any(|a| a == "--serverust-emit-asyncapi"));
    assert!(args.iter().any(|a| a == "asyncapi.yaml"));
}
