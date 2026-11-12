use crate::{Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub root: PathBuf,
    #[serde(default = "checksums")]
    pub checksums: PathBuf,
    #[serde(default = "checksum_signature")]
    pub checksum_signature: PathBuf,
    #[serde(default = "provenance")]
    pub provenance: PathBuf,
    #[serde(default = "provenance_signature")]
    pub provenance_signature: PathBuf,
    #[serde(default)]
    pub public_key: Option<PathBuf>,
}

fn checksums() -> PathBuf {
    "checksums.sha".into()
}
fn checksum_signature() -> PathBuf {
    "checksums.sha.sig".into()
}
fn provenance() -> PathBuf {
    "provenance.json".into()
}
fn provenance_signature() -> PathBuf {
    "provenance.json.sig".into()
}

impl Target {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            checksums: checksums(),
            checksum_signature: checksum_signature(),
            provenance: provenance(),
            provenance_signature: provenance_signature(),
            public_key: None,
        }
    }
    pub fn path(&self, value: &Path) -> PathBuf {
        self.root.join(value)
    }
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub parents: Vec<PathBuf>,
    pub targets: Vec<Target>,
    pub proof_executable: PathBuf,
    pub poll_ms: u64,
    pub verify_interval_ms: u64,
}

impl Config {
    pub fn default_at(cwd: PathBuf) -> Self {
        Self {
            schema: "devtools-watch.config/1".into(),
            parents: vec![cwd],
            targets: Vec::new(),
            proof_executable: "devtool-proof".into(),
            poll_ms: 250,
            verify_interval_ms: 1000,
        }
    }
    pub fn normalize(&mut self, base: &Path) -> Result<()> {
        if self.schema != "devtools-watch.config/1"
            || self.poll_ms == 0
            || self.verify_interval_ms == 0
            || self.proof_executable.as_os_str().is_empty()
        {
            return Err(Error::Invalid("config_invalid"));
        }
        for parent in &mut self.parents {
            *parent = std::path::absolute(base.join(&*parent))?;
        }
        for target in &mut self.targets {
            target.root = std::path::absolute(base.join(&target.root))?;
        }
        if self.proof_executable.components().count() > 1 {
            self.proof_executable = std::path::absolute(base.join(&self.proof_executable))?;
        }
        Ok(())
    }
}

pub struct Paths {
    pub config: PathBuf,
    pub runtime: PathBuf,
}

fn base(variable: &str, suffix: &str) -> Result<PathBuf> {
    match std::env::var_os(variable) {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(
            PathBuf::from(std::env::var_os("HOME").ok_or(Error::Invalid("home_missing"))?)
                .join(suffix),
        ),
    }
}

impl Paths {
    pub fn new() -> Result<Self> {
        Ok(Self {
            config: base("XDG_CONFIG_HOME", ".config")?.join("devtools-watch/config.json"),
            runtime: base("XDG_STATE_HOME", ".local/state")?.join("devtools-watch"),
        })
    }
}

pub fn load(paths: &Paths) -> Result<Config> {
    let cwd = std::env::current_dir()?;
    let mut config = match fs::read(&paths.config) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Config::default_at(cwd.clone())
        }
        Err(error) => return Err(error.into()),
    };
    config.normalize(&cwd)?;
    Ok(config)
}

pub fn save(paths: &Paths, config: &Config) -> Result<()> {
    fs::create_dir_all(paths.config.parent().unwrap())?;
    let mut temporary = tempfile::NamedTempFile::new_in(paths.config.parent().unwrap())?;
    use std::io::Write;
    temporary.write_all(&serde_json::to_vec_pretty(config)?)?;
    temporary
        .persist(&paths.config)
        .map_err(|error| error.error)?;
    Ok(())
}
