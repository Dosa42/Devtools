use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = devtool::cli::Cli::parse();
    if let Err(error) = cli.validate() {
        error.exit();
    }
    match devtool::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("devtool: {error}");
            ExitCode::FAILURE
        }
    }
}
