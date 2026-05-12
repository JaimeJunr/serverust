use clap::Parser;

use serverust_cli::cli::Cli;

fn main() {
    let cli = Cli::parse();
    if let Err(err) = serverust_cli::run(cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
