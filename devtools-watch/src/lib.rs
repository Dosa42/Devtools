pub mod config;
mod daemon;
pub mod state;

use clap::Parser;
use std::fs;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io")]
    Io(#[from] std::io::Error),
    #[error("json")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(&'static str),
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Json(_) => "json",
            Self::Invalid(value) => value,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Parser)]
#[command(disable_help_flag = true, disable_version_flag = true)]
#[group(required = true, multiple = false)]
struct Cli {
    #[arg(long)]
    on: bool,
    #[arg(long)]
    off: bool,
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    config: Option<String>,
}

pub fn execute() -> Result<()> {
    let cli = Cli::try_parse().map_err(|_| Error::Invalid("cli"))?;
    let paths = config::Paths::new()?;
    if cli.on && std::env::var("DEVTOOLS_WATCH_DAEMON").as_deref() == Ok("1") {
        return daemon::worker(&paths);
    }
    let value = if cli.on {
        daemon::on(&paths)?
    } else if cli.off {
        daemon::off(&paths)?
    } else {
        let filename = cli.config.unwrap();
        let mut config = if filename.is_empty() {
            config::load(&paths)?
        } else {
            serde_json::from_slice(&fs::read(filename)?)?
        };
        config.normalize(&std::env::current_dir()?)?;
        config::save(&paths, &config)?;
        let process = daemon::running(&paths);
        serde_json::json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"actions":["--on","--off","--config"],"config_path":paths.config,"runtime_path":paths.runtime,"running":process.is_some(),"process":process,"config":config})
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
