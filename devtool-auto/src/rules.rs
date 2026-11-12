use crate::engine::Engine;
use crate::triggers::{Timer, Watch};
use crate::{Result, absolute, input, print_json, valid_name};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Before,
    After,
    Error,
    Timer,
    Change,
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Before => "before",
            Self::After => "after",
            Self::Error => "error",
            Self::Timer => "timer",
            Self::Change => "change",
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matcher {
    pub field: Field,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Command,
    Flag,
    Path,
    Profile,
    Step,
    Exit,
}

impl Field {
    fn text(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Flag => "flag",
            Self::Path => "path",
            Self::Profile => "profile",
            Self::Step => "step",
            Self::Exit => "exit",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub profile: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchOptions {
    pub recursive: bool,
    pub poll_ms: u64,
    pub debounce_ms: u64,
    pub initial: bool,
    pub runs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub mode: Mode,
    pub cwd: PathBuf,
    pub enabled: bool,
    pub matchers: Vec<Matcher>,
    pub action: Action,
    pub timer: Option<Timer>,
    pub watch: Option<WatchOptions>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    pub schema: String,
    pub rules: Vec<Rule>,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self {
            schema: "devtool-auto.rules/1".into(),
            rules: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub mode: Mode,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub profile: Option<String>,
    pub step: Option<String>,
    pub cwd: PathBuf,
    pub paths: Vec<PathBuf>,
    pub exit_code: Option<u8>,
}

impl Event {
    pub fn environment(&self, rule: &str) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::from([
            ("DEVTOOL_AUTO_RULE".into(), rule.into()),
            ("DEVTOOL_AUTO_MODE".into(), self.mode.to_string()),
            (
                "DEVTOOL_AUTO_COMMAND".into(),
                self.command.clone().unwrap_or_default(),
            ),
            (
                "DEVTOOL_AUTO_PROFILE".into(),
                self.profile.clone().unwrap_or_default(),
            ),
            (
                "DEVTOOL_AUTO_STEP".into(),
                self.step.clone().unwrap_or_default(),
            ),
            ("DEVTOOL_AUTO_CWD".into(), self.cwd.display().to_string()),
            (
                "DEVTOOL_AUTO_EXIT".into(),
                self.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_default(),
            ),
            ("DEVTOOL_AUTO_EVENT".into(), serde_json::to_string(self)?),
        ]))
    }
}

fn glob(pattern: &str, value: &str) -> bool {
    let pattern: Vec<_> = pattern.chars().collect();
    let value: Vec<_> = value.chars().collect();
    let (mut p, mut v, mut star, mut mark) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            mark = v;
        } else if let Some(index) = star {
            p = index + 1;
            mark += 1;
            v = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

fn normalized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

impl Rule {
    pub fn matches(&self, event: &Event) -> bool {
        self.enabled
            && self.mode == event.mode
            && self.matchers.iter().all(|matcher| match matcher.field {
                Field::Command => event.command.as_ref().is_some_and(|command| {
                    glob(&matcher.value, command)
                        || Path::new(command)
                            .file_name()
                            .is_some_and(|name| glob(&matcher.value, &name.to_string_lossy()))
                }),
                Field::Flag => event
                    .args
                    .iter()
                    .take_while(|arg| arg.as_str() != "--")
                    .filter(|arg| arg.starts_with('-'))
                    .any(|arg| {
                        glob(
                            matcher.value.trim_start_matches('-'),
                            arg.split('=').next().unwrap().trim_start_matches('-'),
                        )
                    }),
                Field::Profile => event
                    .profile
                    .as_ref()
                    .is_some_and(|value| glob(&matcher.value, value)),
                Field::Step => event
                    .step
                    .as_ref()
                    .is_some_and(|value| glob(&matcher.value, value)),
                Field::Exit => event
                    .exit_code
                    .is_some_and(|value| glob(&matcher.value, &value.to_string())),
                Field::Path => {
                    let wanted = normalized(&self.cwd.join(&matcher.value));
                    let candidates = if event.paths.is_empty() {
                        vec![event.cwd.clone()]
                    } else {
                        event.paths.clone()
                    };
                    candidates.iter().any(|path| {
                        let path = normalized(path);
                        if matcher.value.contains(['*', '?']) {
                            glob(&wanted.to_string_lossy(), &path.to_string_lossy())
                        } else {
                            path.starts_with(&wanted)
                        }
                    })
                }
            })
    }
}

fn validate(set: &RuleSet) -> Result<()> {
    if set.schema != "devtool-auto.rules/1" {
        return Err(input("Expected schema devtool-auto.rules/1"));
    }
    let mut names = BTreeSet::new();
    for rule in &set.rules {
        valid_name(&rule.name)?;
        valid_name(&rule.action.profile)?;
        if !names.insert(&rule.name) {
            return Err(input(format!("Duplicate rule: {}", rule.name)));
        }
        if !rule.cwd.is_absolute() {
            return Err(input(format!(
                "Rule {} requires an absolute cwd",
                rule.name
            )));
        }
        if rule
            .matchers
            .iter()
            .any(|matcher| matcher.value.is_empty() || matcher.value.contains(['\n', '\r']))
        {
            return Err(input("Matcher values must be nonempty single lines"));
        }
        match rule.mode {
            Mode::Timer => {
                if rule
                    .timer
                    .as_ref()
                    .is_none_or(|timer| timer.interval_ms == 0)
                    || rule.watch.is_some()
                {
                    return Err(input(
                        "Timer mode requires interval > 0 and no watch settings",
                    ));
                }
            }
            Mode::Change => {
                if rule.watch.as_ref().is_none_or(|watch| watch.poll_ms == 0)
                    || rule.timer.is_some()
                    || !rule
                        .matchers
                        .iter()
                        .any(|matcher| matcher.field == Field::Path)
                {
                    return Err(input(
                        "Change mode requires a path matcher, poll > 0 and no timer settings",
                    ));
                }
                if rule.matchers.iter().any(|matcher| {
                    matcher.field == Field::Path && matcher.value.contains(['*', '?'])
                }) {
                    return Err(input("Change path matchers name watched paths directly"));
                }
            }
            _ => {
                if rule.timer.is_some() || rule.watch.is_some() {
                    return Err(input("Timer/watch settings belong to timer/change modes"));
                }
            }
        }
    }
    Ok(())
}

pub fn duration(value: &str) -> Result<u64> {
    let offset = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let number: u64 = value[..offset]
        .parse()
        .map_err(|_| input(format!("Invalid duration: {value}")))?;
    let factor = match &value[offset..] {
        "" | "ms" => 1,
        "s" => 1000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => {
            return Err(input(format!(
                "Duration units: ms, s, m, h, d; got {value}"
            )));
        }
    };
    number
        .checked_mul(factor)
        .ok_or_else(|| input("Duration overflow"))
}

fn matcher(value: &str) -> Result<Matcher> {
    let (field, value) = value.split_once(char::is_whitespace).ok_or_else(|| input("Matcher syntax: command VALUE, flag VALUE, path VALUE, profile VALUE, step VALUE, exit VALUE"))?;
    let field = match field {
        "command" => Field::Command,
        "flag" => Field::Flag,
        "path" => Field::Path,
        "profile" => Field::Profile,
        "step" => Field::Step,
        "exit" => Field::Exit,
        _ => return Err(input(format!("Unknown matcher field: {field}"))),
    };
    Ok(Matcher {
        field,
        value: value.trim().into(),
    })
}

#[derive(Default)]
struct Pending {
    name: String,
    fields: BTreeMap<String, String>,
    matchers: Vec<Matcher>,
}

impl Pending {
    fn finish(self, base: &Path) -> Result<Rule> {
        let required = |key: &str| {
            self.fields
                .get(key)
                .ok_or_else(|| input(format!("Rule {} needs {key}", self.name)))
        };
        let mode = Mode::from_str(required("mode")?, false).map_err(input)?;
        let action = required("action")?
            .strip_prefix("profile ")
            .ok_or_else(|| input("Action syntax: action profile NAME"))?
            .trim()
            .to_owned();
        let number = |key: &str, default: u64| -> Result<u64> {
            self.fields.get(key).map_or(Ok(default), |value| {
                value
                    .parse()
                    .map_err(|_| input(format!("Invalid {key}: {value}")))
            })
        };
        let time = |key: &str, default: u64| {
            self.fields
                .get(key)
                .map_or(Ok(default), |value| duration(value))
        };
        let boolean = |key: &str, default: bool| -> Result<bool> {
            self.fields.get(key).map_or(Ok(default), |value| {
                value
                    .parse()
                    .map_err(|_| input(format!("{key} uses true or false")))
            })
        };
        let allowed = match mode {
            Mode::Timer => vec!["interval", "after", "runs"],
            Mode::Change => vec!["poll", "debounce", "recursive", "initial", "runs"],
            _ => Vec::new(),
        };
        for key in self.fields.keys() {
            if !["mode", "action", "cwd", "enabled"].contains(&key.as_str())
                && !allowed.contains(&key.as_str())
            {
                return Err(input(format!("Setting {key} is not valid for mode {mode}")));
            }
        }
        let cwd = normalized(&base.join(self.fields.get("cwd").map(String::as_str).unwrap_or(".")));
        let timer = match mode {
            Mode::Timer => Some(Timer {
                interval_ms: duration(required("interval")?)?,
                after_ms: time("after", 0)?,
                runs: number("runs", 0)?,
            }),
            _ => None,
        };
        let watch = match mode {
            Mode::Change => Some(WatchOptions {
                recursive: boolean("recursive", true)?,
                poll_ms: time("poll", 250)?,
                debounce_ms: time("debounce", 300)?,
                initial: boolean("initial", false)?,
                runs: number("runs", 0)?,
            }),
            _ => None,
        };
        Ok(Rule {
            name: self.name.clone(),
            mode,
            cwd,
            enabled: boolean("enabled", true)?,
            matchers: self.matchers,
            action: Action { profile: action },
            timer,
            watch,
        })
    }
}

pub fn parse(text: &str, base: &Path) -> Result<RuleSet> {
    let mut set = RuleSet::default();
    let mut pending: Option<Pending> = None;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let value = value.trim();
        let result = (|| -> Result<()> {
            match key {
                "rule" => {
                    if pending.is_some() {
                        return Err(input("Close the current rule with end"));
                    }
                    pending = Some(Pending {
                        name: value.into(),
                        ..Default::default()
                    });
                }
                "end" => {
                    if !value.is_empty() {
                        return Err(input("end has no arguments"));
                    }
                    set.rules.push(
                        pending
                            .take()
                            .ok_or_else(|| input("end outside a rule"))?
                            .finish(base)?,
                    );
                }
                _ => {
                    let item = pending
                        .as_mut()
                        .ok_or_else(|| input("Start with rule NAME"))?;
                    match key {
                        "matcher" => item.matchers.push(matcher(value)?),
                        "mode" | "action" | "cwd" | "enabled" | "interval" | "after" | "runs"
                        | "poll" | "debounce" | "recursive" | "initial" => {
                            if value.is_empty() {
                                return Err(input(format!("{key} requires a value")));
                            }
                            if item.fields.insert(key.into(), value.into()).is_some() {
                                return Err(input(format!("Duplicate setting: {key}")));
                            }
                        }
                        _ => return Err(input(format!("Unknown keyword: {key}"))),
                    }
                }
            }
            Ok(())
        })();
        result.map_err(|error| input(format!("Line {}: {error}", index + 1)))?;
    }
    if pending.is_some() {
        return Err(input("Unclosed rule: missing end"));
    }
    validate(&set)?;
    Ok(set)
}

fn text(set: &RuleSet) -> String {
    let mut result = String::new();
    for rule in &set.rules {
        result.push_str(&format!(
            "rule {}\n  mode {}\n  cwd {}\n  enabled {}\n",
            rule.name,
            rule.mode,
            rule.cwd.display(),
            rule.enabled
        ));
        for matcher in &rule.matchers {
            result.push_str(&format!(
                "  matcher {} {}\n",
                matcher.field.text(),
                matcher.value
            ));
        }
        if let Some(timer) = &rule.timer {
            result.push_str(&format!(
                "  interval {}ms\n  after {}ms\n  runs {}\n",
                timer.interval_ms, timer.after_ms, timer.runs
            ));
        }
        if let Some(watch) = &rule.watch {
            result.push_str(&format!(
                "  recursive {}\n  poll {}ms\n  debounce {}ms\n  initial {}\n  runs {}\n",
                watch.recursive, watch.poll_ms, watch.debounce_ms, watch.initial, watch.runs
            ));
        }
        result.push_str(&format!(
            "  action profile {}\nend\n\n",
            rule.action.profile
        ));
    }
    result
}

fn store_path(root: &Path) -> PathBuf {
    root.join("rules/rules.json")
}

pub(crate) fn load_rules(root: &Path) -> Result<RuleSet> {
    match fs::read(store_path(root)) {
        Ok(bytes) => {
            let set = serde_json::from_slice(&bytes)?;
            validate(&set)?;
            Ok(set)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RuleSet::default()),
        Err(error) => Err(error.into()),
    }
}

fn save_rules(root: &Path, set: &RuleSet) -> Result<()> {
    validate(set)?;
    let path = store_path(root);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_vec_pretty(set)?)?;
    Ok(())
}

fn read_rules(file: &Path) -> Result<RuleSet> {
    let content = fs::read_to_string(file)?;
    if content.trim_start().starts_with('{') {
        let set = serde_json::from_str(&content)?;
        validate(&set)?;
        Ok(set)
    } else {
        parse(&content, &std::env::current_dir()?)
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    Text,
    Json,
}

#[derive(Args)]
pub struct EventArgs {
    #[arg(long, value_enum)]
    pub mode: Mode,
    #[arg(long)]
    pub command: Option<String>,
    #[arg(long = "arg", allow_hyphen_values = true)]
    pub args: Vec<String>,
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub step: Option<String>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long = "path")]
    pub paths: Vec<PathBuf>,
    #[arg(long)]
    pub exit_code: Option<u8>,
}

impl EventArgs {
    fn event(self) -> Result<Event> {
        let cwd = absolute(self.cwd.unwrap_or(std::env::current_dir()?))?;
        Ok(Event {
            mode: self.mode,
            command: self.command,
            args: self.args,
            profile: self.profile,
            step: self.step,
            paths: self
                .paths
                .iter()
                .map(|path| normalized(&cwd.join(path)))
                .collect(),
            cwd,
            exit_code: self.exit_code,
        })
    }
}

#[derive(Args)]
pub struct RuleArgs {
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long, help = "Name of the action profile")]
    action: String,
    #[arg(long = "matcher", help = "Matcher text, for example: command devtool")]
    matchers: Vec<String>,
    #[arg(long)]
    cwd: Option<PathBuf>,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    enabled: bool,
    #[arg(long)]
    interval: Option<String>,
    #[arg(long)]
    after: Option<String>,
    #[arg(long)]
    runs: Option<u64>,
    #[arg(long)]
    poll: Option<String>,
    #[arg(long)]
    debounce: Option<String>,
    #[arg(long, action = clap::ArgAction::Set)]
    recursive: Option<bool>,
    #[arg(long, action = clap::ArgAction::Set)]
    initial: Option<bool>,
}

#[derive(Subcommand)]
pub enum RuleCommand {
    Set {
        name: String,
        #[command(flatten)]
        settings: RuleArgs,
    },
    Import {
        file: PathBuf,
    },
    Export {
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    Check {
        file: PathBuf,
    },
    List,
    Show {
        name: String,
    },
    Remove {
        name: String,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    Start {
        name: String,
        #[arg(long)]
        runs: Option<u64>,
    },
    Match {
        #[command(flatten)]
        event: EventArgs,
    },
    Emit {
        #[command(flatten)]
        event: EventArgs,
    },
    Schema,
}

pub fn execute(root: &Path, command: RuleCommand) -> Result<u8> {
    match command {
        RuleCommand::Set { name, settings } => {
            let mut item = Pending {
                name,
                matchers: settings
                    .matchers
                    .iter()
                    .map(|value| matcher(value))
                    .collect::<Result<_>>()?,
                ..Default::default()
            };
            item.fields.insert("mode".into(), settings.mode.to_string());
            item.fields
                .insert("action".into(), format!("profile {}", settings.action));
            item.fields
                .insert("enabled".into(), settings.enabled.to_string());
            if let Some(cwd) = settings.cwd {
                item.fields.insert("cwd".into(), cwd.display().to_string());
            }
            for (key, value) in [
                ("interval", settings.interval),
                ("after", settings.after),
                ("runs", settings.runs.map(|value| value.to_string())),
                ("poll", settings.poll),
                ("debounce", settings.debounce),
                (
                    "recursive",
                    settings.recursive.map(|value| value.to_string()),
                ),
                ("initial", settings.initial.map(|value| value.to_string())),
            ] {
                if let Some(value) = value {
                    item.fields.insert(key.into(), value);
                }
            }
            let rule = item.finish(&std::env::current_dir()?)?;
            let mut set = load_rules(root)?;
            match set.rules.iter().position(|old| old.name == rule.name) {
                Some(index) => set.rules[index] = rule,
                None => set.rules.push(rule),
            }
            save_rules(root, &set)?;
        }
        RuleCommand::Import { file } => {
            let imported = read_rules(&file)?;
            let mut set = load_rules(root)?;
            for rule in imported.rules {
                match set.rules.iter().position(|old| old.name == rule.name) {
                    Some(index) => set.rules[index] = rule,
                    None => set.rules.push(rule),
                }
            }
            save_rules(root, &set)?;
        }
        RuleCommand::Export { output, format } => {
            let set = load_rules(root)?;
            let content = match format {
                Format::Text => text(&set),
                Format::Json => serde_json::to_string_pretty(&set)? + "\n",
            };
            match output {
                Some(path) => fs::write(path, content)?,
                None => print!("{content}"),
            }
        }
        RuleCommand::Check { file } => {
            let set = read_rules(&file)?;
            print_json(&serde_json::json!({"valid":true,"rules":set.rules.len()}))?;
        }
        RuleCommand::List => print_json(
            &load_rules(root)?
                .rules
                .iter()
                .map(|rule| &rule.name)
                .collect::<Vec<_>>(),
        )?,
        RuleCommand::Show { name } => {
            let set = load_rules(root)?;
            print_json(
                set.rules
                    .iter()
                    .find(|rule| rule.name == name)
                    .ok_or_else(|| input(format!("Unknown rule: {name}")))?,
            )?;
        }
        RuleCommand::Remove { name } => {
            let mut set = load_rules(root)?;
            let index = set
                .rules
                .iter()
                .position(|rule| rule.name == name)
                .ok_or_else(|| input(format!("Unknown rule: {name}")))?;
            set.rules.remove(index);
            save_rules(root, &set)?;
        }
        RuleCommand::Enable { name } => enabled(root, &name, true)?,
        RuleCommand::Disable { name } => enabled(root, &name, false)?,
        RuleCommand::Start { name, runs } => return start(root, &name, runs),
        RuleCommand::Match { event } => {
            let event = event.event()?;
            print_json(
                &load_rules(root)?
                    .rules
                    .iter()
                    .filter(|rule| rule.matches(&event))
                    .map(|rule| &rule.name)
                    .collect::<Vec<_>>(),
            )?;
        }
        RuleCommand::Emit { event } => return Engine::new(root)?.emit(&event.event()?),
        RuleCommand::Schema => println!("{}", include_str!("../rules.schema.json")),
    }
    Ok(0)
}

fn enabled(root: &Path, name: &str, enabled: bool) -> Result<()> {
    let mut set = load_rules(root)?;
    set.rules
        .iter_mut()
        .find(|rule| rule.name == name)
        .ok_or_else(|| input(format!("Unknown rule: {name}")))?
        .enabled = enabled;
    save_rules(root, &set)
}

fn start(root: &Path, name: &str, runs: Option<u64>) -> Result<u8> {
    let set = load_rules(root)?;
    let rule = set
        .rules
        .iter()
        .find(|rule| rule.name == name)
        .ok_or_else(|| input(format!("Unknown rule: {name}")))?;
    if !rule.enabled {
        return Err(input(format!("Rule {name} is disabled")));
    }
    let mut engine = Engine::new(root)?;
    let event = Event {
        mode: rule.mode,
        command: None,
        args: Vec::new(),
        profile: Some(rule.action.profile.clone()),
        step: None,
        cwd: rule.cwd.clone(),
        paths: Vec::new(),
        exit_code: None,
    };
    match rule.mode {
        Mode::Timer => {
            let settings = rule.timer.as_ref().unwrap();
            let limit = runs.unwrap_or(settings.runs);
            sleep(Duration::from_millis(settings.after_ms));
            let mut count = 0;
            loop {
                count += 1;
                let code = engine.named(name, &event)?;
                if code != 0 || (limit != 0 && count >= limit) {
                    return Ok(code);
                }
                sleep(Duration::from_millis(settings.interval_ms));
            }
        }
        Mode::Change => {
            let settings = rule.watch.as_ref().unwrap();
            let paths = rule
                .matchers
                .iter()
                .filter(|matcher| matcher.field == Field::Path)
                .map(|matcher| PathBuf::from(&matcher.value))
                .collect();
            crate::triggers::watch_events(
                &rule.cwd,
                Watch {
                    paths,
                    recursive: settings.recursive,
                    poll_ms: settings.poll_ms,
                    debounce_ms: settings.debounce_ms,
                    initial: settings.initial,
                    runs: runs.unwrap_or(settings.runs),
                },
                |paths| {
                    let mut event = event.clone();
                    event.paths = paths;
                    engine.named(name, &event)
                },
            )
        }
        _ => Err(input(
            "rule start is for timer/change; command modes activate through run, exec or rule emit",
        )),
    }
}
