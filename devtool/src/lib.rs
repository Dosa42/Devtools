pub mod archive;
pub mod canonical;
pub mod cli;
pub mod config;
pub mod error;
pub mod filesystem;
pub mod menu;
pub mod schema;

use cli::{Cli, Command, ConfigCommand, FileCommand, Parents};
use config::{ArchiveFormat, ColorMode, OutputFormat, Settings};
use error::{Result, io};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};

fn emit(value: Value, text: &str, settings: &Settings) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    if settings.output.format == OutputFormat::Json {
        serde_json::to_writer_pretty(&mut stdout, &value)?;
        writeln!(stdout).map_err(|error| io("Write output", Path::new("stdout"), error))?;
    } else {
        let color = match settings.output.color {
            ColorMode::Always => true,
            ColorMode::Auto => stdout.is_terminal() && std::env::var_os("NO_COLOR").is_none(),
            ColorMode::Never => false,
        };
        if color {
            writeln!(stdout, "\u{1b}[32m{text}\u{1b}[0m")
        } else {
            writeln!(stdout, "{text}")
        }
        .map_err(|error| io("Write output", Path::new("stdout"), error))?;
    }
    Ok(())
}

fn raw_schema(value: Value, output: Option<&Path>) -> Result<()> {
    if let Some(path) = output {
        filesystem::write_json(path, &value)
    } else {
        let mut settings = Settings::default();
        settings.output.format = OutputFormat::Json;
        emit(value, "", &settings)
    }
}

fn global_overrides(cli: &Cli) -> BTreeMap<String, String> {
    let mut overrides = BTreeMap::new();
    if cli.json {
        overrides.insert("output.format".into(), "json".into());
    }
    if let Some(color) = cli.color {
        overrides.insert(
            "output.color".into(),
            serde_json::to_value(color)
                .expect("Color is serializable")
                .as_str()
                .expect("Color is a string")
                .to_owned(),
        );
    }
    overrides
}

fn parent_override(overrides: &mut BTreeMap<String, String>, parents: &Parents) {
    if parents.parents || parents.no_parents {
        overrides.insert(
            "filesystem.create_parents".into(),
            parents.parents.to_string(),
        );
    }
}

fn explicit_output(settings: &mut Settings, cli: &Cli) {
    if cli.json {
        settings.output.format = OutputFormat::Json;
    }
    if let Some(color) = cli.color {
        settings.output.color = color;
    }
}

fn default_archive(source: &Path, format: ArchiveFormat) -> Result<PathBuf> {
    let source =
        std::fs::canonicalize(source).map_err(|error| io("Read archive source", source, error))?;
    let mut name = source
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("root"))
        .to_os_string();
    name.push(format!(".{}", format.extension()));
    Ok(source.with_file_name(name))
}

pub fn run(cli: Cli) -> Result<()> {
    cli.validate()
        .map_err(|error| error::Error::Message(error.to_string()))?;
    if cli.directory {
        let (path, required) = config::path(cli.config.as_deref())?;
        let effective = config::load(&path, required, &global_overrides(&cli))?;
        let parent = cli.path.as_deref().unwrap_or(Path::new("."));
        let directory = filesystem::create_root(parent)?;
        let text = directory.path.display().to_string();
        return emit(serde_json::to_value(directory)?, &text, &effective.settings);
    }
    if cli.canonical_tree {
        let (path, required) = config::path(cli.config.as_deref())?;
        let effective = config::load(&path, required, &global_overrides(&cli))?;
        let target = match &cli.path {
            Some(path) => path.clone(),
            None => filesystem::latest_root()?,
        };
        let report = canonical::apply(&target)?;
        let text = format!(
            "Canonical tree: {} ({} directories, {} files)",
            report.path.display(),
            report.directories.len(),
            report.files.len()
        );
        return emit(serde_json::to_value(report)?, &text, &effective.settings);
    }
    let command = cli.command.as_ref().ok_or_else(|| {
        error::Error::Message("Choose -D, --canonical-tree or a subcommand".into())
    })?;
    if let Command::Schema { output } = command {
        return raw_schema(schema::cli(), output.as_deref());
    }
    if let Command::Config {
        command: ConfigCommand::Schema { output },
    } = command
    {
        return raw_schema(schema::settings(), output.as_deref());
    }
    let (path, required) = config::path(cli.config.as_deref())?;
    let mut overrides = global_overrides(&cli);
    let mut basic = Settings::default();
    explicit_output(&mut basic, &cli);
    if let Command::Config { command } = command {
        match command {
            ConfigCommand::Path => {
                return emit(json!({ "path": path }), &path.display().to_string(), &basic);
            }
            ConfigCommand::Menu => {
                if cli.json {
                    return Err(error::Error::Message(
                        "config menu is interactive; omit --json".into(),
                    ));
                }
                return menu::run(
                    &path,
                    &mut std::io::stdin().lock(),
                    &mut std::io::stdout().lock(),
                );
            }
            ConfigCommand::Init { force } => {
                config::save(&path, &Settings::default(), *force)?;
                return emit(
                    json!({ "path": path, "initialized": true }),
                    &format!("Created {}", path.display()),
                    &basic,
                );
            }
            ConfigCommand::Set { key, value } => {
                let (mut settings, _) = config::stored(&path, false)?;
                config::set(&mut settings, key, value)?;
                config::save(&path, &settings, true)?;
                let saved = config::get(&serde_json::to_value(&settings)?, key)?;
                explicit_output(&mut settings, &cli);
                return emit(
                    json!({ "path": path, "key": key, "value": saved }),
                    &format!("Saved {key} = {value}"),
                    &settings,
                );
            }
            ConfigCommand::Show { effective: false } => {
                let (mut settings, _) = config::stored(&path, required)?;
                let value = serde_json::to_value(&settings)?;
                let text = settings.toml()?;
                explicit_output(&mut settings, &cli);
                return emit(value, text.trim_end(), &settings);
            }
            ConfigCommand::Validate => {
                let (mut settings, _) = config::stored(&path, true)?;
                explicit_output(&mut settings, &cli);
                return emit(
                    json!({ "path": path, "valid": true }),
                    &format!("Valid configuration: {}", path.display()),
                    &settings,
                );
            }
            _ => {}
        }
    }
    match command {
        Command::Mkdir(args) => parent_override(&mut overrides, &args.parents),
        Command::File {
            command: FileCommand::Create(args),
        } => {
            parent_override(&mut overrides, &args.parents);
            if args.overwrite || args.append {
                overrides.insert(
                    "filesystem.existing_file".into(),
                    if args.append { "append" } else { "overwrite" }.into(),
                );
            }
        }
        Command::Compress(args) => {
            if let Some(format) = args.format {
                overrides.insert("compression.format".into(), format.extension().into());
            }
            if let Some(level) = args.level {
                overrides.insert("compression.level".into(), level.to_string());
            }
        }
        _ => {}
    }
    let effective = config::load(&path, required, &overrides)?;
    let settings = &effective.settings;
    match command {
        Command::Mkdir(args) => {
            for path in &args.paths {
                filesystem::mkdir(path, settings.filesystem.create_parents)?;
            }
            emit(
                json!({ "directories": args.paths }),
                &format!(
                    "Created {}",
                    args.paths
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                settings,
            )
        }
        Command::File {
            command: FileCommand::Create(args),
        } => {
            let bytes = if args.stdin {
                filesystem::create(
                    &args.path,
                    &mut std::io::stdin().lock(),
                    settings.filesystem.create_parents,
                    settings.filesystem.existing_file,
                )?
            } else {
                let content = args.content.as_deref().unwrap_or("");
                filesystem::create(
                    &args.path,
                    &mut content.as_bytes(),
                    settings.filesystem.create_parents,
                    settings.filesystem.existing_file,
                )?
            };
            emit(
                json!({ "path": args.path, "bytes_written": bytes, "mode": settings.filesystem.existing_file }),
                &format!("Wrote {bytes} bytes to {}", args.path.display()),
                settings,
            )
        }
        Command::Compress(args) => {
            let output = match &args.output {
                Some(path) => path.clone(),
                None => default_archive(&args.source, settings.compression.format)?,
            };
            let report = archive::compress(
                &args.source,
                &output,
                settings.compression.format,
                settings.compression.level,
            )?;
            let text = format!(
                "Created {} ({} entries, {} bytes)",
                report.path.display(),
                report.entries,
                report.bytes
            );
            emit(serde_json::to_value(report)?, &text, settings)
        }
        Command::Config {
            command: ConfigCommand::Show { effective: true },
        } => {
            let mut text = settings.toml()?;
            text.push_str(&format!("\nConfiguration: {}\nSources:\n", path.display()));
            for (key, source) in &effective.sources {
                text.push_str(&format!("  {key}: {source}\n"));
            }
            emit(serde_json::to_value(&effective)?, text.trim_end(), settings)
        }
        Command::Config {
            command: ConfigCommand::Get { key },
        } => {
            let value = config::get(&serde_json::to_value(settings)?, key)?;
            let text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            emit(value, &text, settings)
        }
        _ => unreachable!(),
    }
}
