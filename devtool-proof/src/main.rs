use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    match devtool_proof::execute(devtool_proof::cli::Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("devtool-proof: {error}");
            ExitCode::from(2)
        }
    }
}
