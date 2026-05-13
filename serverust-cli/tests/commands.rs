use serverust_cli::cli::{Arch, OpenapiClientLang};
use serverust_cli::commands;

#[test]
fn build_command_program_and_args() {
    let cmd = commands::build_cargo_command(false);
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(program, "cargo");
    assert_eq!(args, vec!["build"]);

    let cmd = commands::build_cargo_command(true);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert!(args.contains(&"build".to_string()));
    assert!(args.contains(&"--release".to_string()));
}

#[test]
fn dev_command_uses_cargo_watch() {
    let cmd = commands::dev_cargo_command();
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(program, "cargo");
    assert_eq!(args.first().map(String::as_str), Some("watch"));
    assert!(args.contains(&"run".to_string()));
}

#[test]
fn deploy_lambda_command_includes_arch() {
    let cmd = commands::deploy_lambda_cargo_command(Arch::Arm64);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(args.first().map(String::as_str), Some("lambda"));
    assert!(args.contains(&"--arm64".to_string()));

    let cmd = commands::deploy_lambda_cargo_command(Arch::X86_64);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert!(args.contains(&"--x86-64".to_string()));
}

#[test]
fn openapi_export_command_passes_out_path() {
    let cmd = commands::openapi_export_command(std::path::Path::new("spec.json"));
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert!(args.iter().any(|a| a == "run"));
    assert!(args.iter().any(|a| a == "spec.json"));
}

#[test]
fn openapi_client_command_maps_lang_and_paths() {
    let cmd = commands::openapi_client_command(
        OpenapiClientLang::Ts,
        std::path::Path::new("openapi.json"),
        std::path::Path::new("sdk/ts"),
    );
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(program, "openapi-generator-cli");
    assert!(args.contains(&"generate".to_string()));
    assert!(args.contains(&"typescript-fetch".to_string()));
    assert!(args.contains(&"openapi.json".to_string()));
    assert!(args.contains(&"sdk/ts".to_string()));
}

#[test]
fn info_text_mentions_versions() {
    let s = commands::info_text();
    assert!(s.contains("serverust-cli"));
    assert!(s.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn doctor_report_mentions_core_files() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").expect("cargo");
    std::fs::write(
        dir.path().join("serverust.toml"),
        "[default.server]\nport=3000\n[default.telemetry]\nformat=\"json\"\n[default.openapi]\ntitle=\"x\"\n",
    )
    .expect("cfg");
    let report = commands::doctor_report(dir.path());
    assert!(report.contains("Cargo.toml"));
    assert!(report.contains("serverust.toml"));
    assert!(report.contains("[default.telemetry]"));
    assert!(report.contains("[default.openapi]"));
}
