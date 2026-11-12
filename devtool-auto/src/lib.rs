pub mod cli;
mod engine;
mod lab;
pub mod rules;
mod triggers;

use cli::{Cli, Command as Action, ProfileCommand, Settings, StepCommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Input(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub name: String,
    pub cwd: PathBuf,
    pub repeat: u64,
    pub interval_ms: u64,
    pub env: BTreeMap<String, String>,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub timer: Option<triggers::Timer>,
    #[serde(default)]
    pub watch: Option<triggers::Watch>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub name: String,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub delay_ms: u64,
    pub action: StepAction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepAction {
    Exec { program: String, args: Vec<String> },
    Shell { command: String },
}

fn input(message: impl Into<String>) -> Error {
    Error::Input(message.into())
}

fn valid_name(name: &str) -> Result<()> {
    match !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        true => Ok(()),
        false => Err(input(
            "Profile names use letters, numbers, dots, underscores and hyphens",
        )),
    }
}

fn directory(explicit: Option<PathBuf>) -> Result<PathBuf> {
    match explicit {
        Some(path) => Ok(path),
        None => {
            let root = match std::env::var_os("XDG_CONFIG_HOME") {
                Some(path) => PathBuf::from(path),
                None => PathBuf::from(
                    std::env::var_os("HOME")
                        .ok_or_else(|| input("HOME is not set; use --profiles-dir"))?,
                )
                .join(".config"),
            };
            Ok(root.join("devtool-auto/profiles"))
        }
    }
}

fn profile_path(root: &Path, name: &str) -> Result<PathBuf> {
    valid_name(name)?;
    Ok(root.join(format!("{name}.json")))
}

fn validate(profile: &Profile) -> Result<()> {
    valid_name(&profile.name)?;
    if profile
        .timer
        .as_ref()
        .is_some_and(|timer| timer.interval_ms == 0)
    {
        return Err(input("Timer interval_ms must be >= 1"));
    }
    if profile
        .watch
        .as_ref()
        .is_some_and(|watch| watch.poll_ms == 0 || watch.paths.is_empty())
    {
        return Err(input("Watcher requires paths and poll_ms >= 1"));
    }
    match (profile.schema_version, profile.repeat) {
        (1, 1..) => Ok(()),
        _ => Err(input("Profiles require schema_version 1 and repeat >= 1")),
    }
}

fn load(root: &Path, name: &str) -> Result<Profile> {
    let profile = serde_json::from_slice(&fs::read(profile_path(root, name)?)?)?;
    validate(&profile)?;
    Ok(profile)
}

fn save(root: &Path, profile: &Profile, create: bool) -> Result<()> {
    validate(profile)?;
    fs::create_dir_all(root)?;
    let bytes = serde_json::to_vec_pretty(profile)?;
    let mut options = fs::OpenOptions::new();
    options.write(true);
    match create {
        true => options.create_new(true),
        false => options.truncate(true),
    };
    let mut file = options.open(profile_path(root, &profile.name)?)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn absolute(path: PathBuf) -> Result<PathBuf> {
    Ok(std::path::absolute(path)?)
}

fn environment(values: Vec<String>) -> Result<BTreeMap<String, String>> {
    values
        .into_iter()
        .map(|value| {
            let (key, content) = value
                .split_once('=')
                .ok_or_else(|| input("--env requires KEY=VALUE"))?;
            match key.is_empty() || key.contains('\0') || content.contains('\0') {
                true => Err(input("Invalid environment variable")),
                false => Ok((key.to_owned(), content.to_owned())),
            }
        })
        .collect()
}

fn configure(profile: &mut Profile, settings: Settings) -> Result<()> {
    if let Some(cwd) = settings.cwd {
        profile.cwd = absolute(cwd)?;
    }
    if let Some(repeat) = settings.repeat {
        profile.repeat = repeat;
    }
    if let Some(interval) = settings.interval_ms {
        profile.interval_ms = interval;
    }
    profile.env.extend(environment(settings.env)?);
    for key in settings.unset_env {
        profile.env.remove(&key);
    }
    Ok(())
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn run(
    root: &Path,
    profile: Profile,
    cwd: Option<PathBuf>,
    repeat: Option<u64>,
    interval: Option<u64>,
) -> Result<u8> {
    engine::Engine::new(root)?.profile(profile, cwd, repeat, interval)
}

pub fn execute(cli: Cli) -> Result<u8> {
    let root = directory(cli.profiles_dir)?;
    match cli.command {
        Action::Profile { command } => match command {
            ProfileCommand::Create { name, settings } => {
                let mut profile = Profile {
                    schema_version: 1,
                    name,
                    cwd: std::env::current_dir()?,
                    repeat: 1,
                    interval_ms: 0,
                    env: BTreeMap::new(),
                    steps: Vec::new(),
                    timer: None,
                    watch: None,
                };
                configure(&mut profile, settings)?;
                save(&root, &profile, true)?;
                println!("{}", profile_path(&root, &profile.name)?.display());
            }
            ProfileCommand::Set { name, settings } => {
                let mut profile = load(&root, &name)?;
                configure(&mut profile, settings)?;
                save(&root, &profile, false)?;
            }
            ProfileCommand::List => {
                let entries = match fs::read_dir(&root) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return print_json(&Vec::<String>::new()).map(|()| 0);
                    }
                    Err(error) => return Err(error.into()),
                };
                let mut names = Vec::new();
                for entry in entries {
                    let path = entry?.path();
                    if path.extension().and_then(|value| value.to_str()) == Some("json") {
                        names.push(path.file_stem().unwrap().to_string_lossy().into_owned());
                    }
                }
                names.sort();
                print_json(&names)?;
            }
            ProfileCommand::Show { name } => print_json(&load(&root, &name)?)?,
            ProfileCommand::Export { name, output } => {
                fs::write(output, serde_json::to_vec_pretty(&load(&root, &name)?)?)?
            }
            ProfileCommand::Import { file, name } => {
                let mut profile: Profile = serde_json::from_slice(&fs::read(file)?)?;
                if let Some(name) = name {
                    profile.name = name;
                }
                save(&root, &profile, true)?;
            }
            ProfileCommand::Remove { name } => fs::remove_file(profile_path(&root, &name)?)?,
        },
        Action::Step { command } => match command {
            StepCommand::Add {
                profile,
                name,
                program,
                args,
                command,
                cwd,
                env,
                delay_ms,
            } => {
                let mut profile = load(&root, &profile)?;
                let action = match (program, command) {
                    (Some(program), None) => StepAction::Exec { program, args },
                    (None, Some(command)) => StepAction::Shell { command },
                    _ => return Err(input("Choose --program or --command")),
                };
                profile.steps.push(Step {
                    name,
                    action,
                    cwd,
                    env: environment(env)?,
                    delay_ms,
                });
                save(&root, &profile, false)?;
            }
            StepCommand::List { profile } => print_json(&load(&root, &profile)?.steps)?,
            StepCommand::Remove { profile, index } => {
                let mut profile = load(&root, &profile)?;
                let index = index
                    .checked_sub(1)
                    .filter(|index| *index < profile.steps.len())
                    .ok_or_else(|| input("--index must be an existing 1-based step number"))?;
                profile.steps.remove(index);
                save(&root, &profile, false)?;
            }
            StepCommand::Clear { profile } => {
                let mut profile = load(&root, &profile)?;
                profile.steps.clear();
                save(&root, &profile, false)?;
            }
        },
        Action::Run {
            name,
            cwd,
            repeat,
            interval_ms,
        } => return run(&root, load(&root, &name)?, cwd, repeat, interval_ms),
        Action::Timer { command } => return triggers::timer(&root, command),
        Action::Watch { command } => return triggers::watch(&root, command),
        Action::Lab {
            command: cli::LabCommand::Init { path },
        } => lab::init(&root, absolute(path)?)?,
        Action::Rule { command } => return rules::execute(&root, command),
        Action::Exec { cwd, command } => {
            let cwd = absolute(cwd.unwrap_or(std::env::current_dir()?))?;
            return engine::Engine::new(&root)?.external(&command[0], &command[1..], cwd);
        }
        Action::Schema => println!("{}", include_str!("../profile.schema.json")),
    }
    Ok(0)
}
