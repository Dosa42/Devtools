use std::process::ExitCode;

fn main() -> ExitCode {
    match devtools_watch::execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            println!("{}", serde_json::json!({"ok":false,"error":error.code()}));
            ExitCode::from(2)
        }
    }
}
