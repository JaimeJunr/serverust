use clap::Parser;
use serverust_cli::cli::{Arch, Cli, Command, DeployTarget, GenerateKind};

#[test]
fn parses_new_command() {
    let cli = Cli::try_parse_from(["serverust", "new", "myapp"]).expect("parse");
    match cli.command {
        Command::New { name } => assert_eq!(name, "myapp"),
        other => panic!("expected New, got {other:?}"),
    }
}

#[test]
fn parses_generate_each_kind() {
    let cases = [
        ("resource", GenerateKind::Resource),
        ("module", GenerateKind::Module),
        ("controller", GenerateKind::Controller),
        ("service", GenerateKind::Service),
        ("pipe", GenerateKind::Pipe),
        ("guard", GenerateKind::Guard),
        ("interceptor", GenerateKind::Interceptor),
        ("filter", GenerateKind::Filter),
    ];
    for (raw, expected) in cases {
        let cli = Cli::try_parse_from(["serverust", "generate", raw, "users"])
            .unwrap_or_else(|_| panic!("parse generate {raw}"));
        match cli.command {
            Command::Generate { kind, name } => {
                assert_eq!(kind, expected, "kind for {raw}");
                assert_eq!(name, "users");
            }
            other => panic!("expected Generate, got {other:?}"),
        }
    }
}

#[test]
fn parses_dev_command() {
    let cli = Cli::try_parse_from(["serverust", "dev"]).expect("parse");
    assert!(matches!(cli.command, Command::Dev));
}

#[test]
fn parses_build_default_and_release() {
    let cli = Cli::try_parse_from(["serverust", "build"]).expect("parse");
    match cli.command {
        Command::Build { release } => assert!(!release),
        other => panic!("expected Build, got {other:?}"),
    }
    let cli = Cli::try_parse_from(["serverust", "build", "--release"]).expect("parse");
    match cli.command {
        Command::Build { release } => assert!(release),
        other => panic!("expected Build, got {other:?}"),
    }
}

#[test]
fn parses_deploy_lambda_with_arch() {
    let cli = Cli::try_parse_from(["serverust", "deploy", "lambda"]).expect("parse");
    match cli.command {
        Command::Deploy { target } => match target {
            DeployTarget::Lambda { arch } => assert_eq!(arch, Arch::Arm64),
        },
        other => panic!("expected Deploy, got {other:?}"),
    }

    let cli =
        Cli::try_parse_from(["serverust", "deploy", "lambda", "--arch", "x86_64"]).expect("parse");
    match cli.command {
        Command::Deploy { target } => match target {
            DeployTarget::Lambda { arch } => assert_eq!(arch, Arch::X86_64),
        },
        other => panic!("expected Deploy, got {other:?}"),
    }
}

#[test]
fn parses_info() {
    let cli = Cli::try_parse_from(["serverust", "info"]).expect("parse");
    assert!(matches!(cli.command, Command::Info));
}

#[test]
fn parses_openapi_with_out() {
    let cli = Cli::try_parse_from(["serverust", "openapi", "--out", "openapi.json"]).expect("parse");
    match cli.command {
        Command::Openapi { out } => assert_eq!(out.as_os_str(), "openapi.json"),
        other => panic!("expected Openapi, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_generate_kind() {
    let result = Cli::try_parse_from(["serverust", "generate", "weirdo", "x"]);
    assert!(result.is_err(), "unknown kind should be rejected");
}
