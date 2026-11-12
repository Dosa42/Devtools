use crate::error::{Error, Result, io};
use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const KEYS: &[&str] = &[
    "schema_version",
    "filesystem.create_parents",
    "filesystem.existing_file",
    "compression.format",
    "compression.level",
    "output.format",
    "output.color",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
pub enum ArchiveFormat {
    #[serde(rename = "7z")]
    #[value(name = "7z")]
    SevenZ,
    #[serde(rename = "tar.gz")]
    #[value(name = "tar.gz", alias = "tgz")]
    TarGz,
}

impl ArchiveFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::SevenZ => "7z",
            Self::TarGz => "tar.gz",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExistingFile {
    Error,
    Overwrite,
    Append,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Filesystem {
    pub create_parents: bool,
    pub existing_file: ExistingFile,
}

impl Default for Filesystem {
    fn default() -> Self {
        Self {
            create_parents: true,
            existing_file: ExistingFile::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Compression {
    pub format: ArchiveFormat,
    #[schemars(range(min = 0, max = 9))]
    pub level: u8,
}

impl Default for Compression {
    fn default() -> Self {
        Self {
            format: ArchiveFormat::TarGz,
            level: 6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    pub format: OutputFormat,
    pub color: ColorMode,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            format: OutputFormat::Text,
            color: ColorMode::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub filesystem: Filesystem,
    pub compression: Compression,
    pub output: Output,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            filesystem: Filesystem::default(),
            compression: Compression::default(),
            output: Output::default(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::Config("schema_version must be 1".into()));
        }
        if self.compression.level > 9 {
            return Err(Error::Config(
                "compression.level must be between 0 and 9".into(),
            ));
        }
        Ok(())
    }

    pub fn toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|error| Error::Config(error.to_string()))
    }
}

#[derive(Debug, Serialize)]
pub struct Effective {
    pub config_path: PathBuf,
    pub settings: Settings,
    pub sources: BTreeMap<String, String>,
}

pub fn path(explicit: Option<&Path>) -> Result<(PathBuf, bool)> {
    if let Some(path) = explicit {
        return Ok((path.to_owned(), true));
    }
    if let Some(path) = env::var_os("DEVTOOL_CONFIG") {
        return Ok((PathBuf::from(path), true));
    }
    let root = env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| {
            Error::Config("HOME or XDG_CONFIG_HOME must be set, or use --config".into())
        })?;
    Ok((root.join("devtool/config.toml"), false))
}

pub fn stored(path: &Path, required: bool) -> Result<(Settings, Value)> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let settings: Settings = toml::from_str(&text)
                .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
            settings.validate()?;
            let raw: toml::Value =
                toml::from_str(&text).map_err(|error| Error::Config(error.to_string()))?;
            Ok((settings, serde_json::to_value(raw)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => {
            Ok((Settings::default(), serde_json::json!({})))
        }
        Err(error) => Err(io("Read configuration", path, error)),
    }
}

pub fn get(value: &Value, key: &str) -> Result<Value> {
    if !KEYS.contains(&key) {
        return Err(Error::Config(format!(
            "Unknown setting '{key}'; available keys: {}",
            KEYS.join(", ")
        )));
    }
    let pointer = format!("/{}", key.replace('.', "/"));
    value
        .pointer(&pointer)
        .cloned()
        .ok_or_else(|| Error::Config(format!("Missing setting '{key}'")))
}

pub fn set(settings: &mut Settings, key: &str, text: &str) -> Result<()> {
    let mut value = serde_json::to_value(&*settings)?;
    let previous = get(&value, key)?;
    let parsed = match previous {
        Value::Bool(_) => Value::Bool(
            text.parse()
                .map_err(|_| Error::Config(format!("{key} expects true or false")))?,
        ),
        Value::Number(_) => Value::Number(
            text.parse::<u64>()
                .map_err(|_| Error::Config(format!("{key} expects a non-negative integer")))?
                .into(),
        ),
        _ => Value::String(text.to_owned()),
    };
    let pointer = format!("/{}", key.replace('.', "/"));
    *value
        .pointer_mut(&pointer)
        .ok_or_else(|| Error::Config(format!("Unknown setting '{key}'")))? = parsed;
    let next: Settings =
        serde_json::from_value(value).map_err(|error| Error::Config(format!("{key}: {error}")))?;
    next.validate()?;
    *settings = next;
    Ok(())
}

pub fn load(
    path: &Path,
    required: bool,
    overrides: &BTreeMap<String, String>,
) -> Result<Effective> {
    let (mut settings, raw) = stored(path, required)?;
    let mut sources = BTreeMap::new();
    for key in KEYS {
        let pointer = format!("/{}", key.replace('.', "/"));
        let source = if raw.pointer(&pointer).is_some() {
            "file"
        } else {
            "default"
        };
        sources.insert((*key).to_owned(), source.to_owned());
        let variable = format!("DEVTOOL_{}", key.replace('.', "_").to_uppercase());
        if let Some(text) = env::var_os(&variable) {
            let text = text
                .into_string()
                .map_err(|_| Error::Config(format!("{variable} must contain UTF-8")))?;
            set(&mut settings, key, &text)
                .map_err(|error| Error::Config(format!("{variable}: {error}")))?;
            sources.insert((*key).to_owned(), format!("env:{variable}"));
        }
    }
    for (key, text) in overrides {
        set(&mut settings, key, text)?;
        sources.insert(key.to_owned(), "cli".into());
    }
    Ok(Effective {
        config_path: path.to_owned(),
        settings,
        sources,
    })
}

pub fn save(path: &Path, settings: &Settings, overwrite: bool) -> Result<()> {
    settings.validate()?;
    let text = settings.toml()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| io("Create configuration directory", parent, error))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| io("Create configuration", path, error))?;
    temporary
        .write_all(text.as_bytes())
        .map_err(|error| io("Write configuration", path, error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| io("Flush configuration", path, error))?;
    if overwrite {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    }
    .map_err(|error| io("Save configuration", path, error.error))?;
    Ok(())
}
