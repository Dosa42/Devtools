use clap::Parser;
use devtool_auto::{cli::Cli, execute};
use std::process::ExitCode;

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("devtool-auto: {error}");
            ExitCode::FAILURE
        }
    }
}
