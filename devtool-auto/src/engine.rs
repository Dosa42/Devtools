use crate::rules::{Event, Mode, RuleSet};
use crate::{Profile, Result, StepAction, absolute, input, load};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread::sleep;
use std::time::Duration;

pub(crate) struct Engine {
    root: PathBuf,
    rules: RuleSet,
    active: Vec<String>,
}

impl Engine {
    pub fn new(root: &Path) -> Result<Self> {
        Ok(Self {
            root: root.to_owned(),
            rules: crate::rules::load_rules(root)?,
            active: Vec::new(),
        })
    }

    pub fn emit(&mut self, event: &Event) -> Result<u8> {
        let matches: Vec<_> = self
            .rules
            .rules
            .iter()
            .filter(|rule| rule.matches(event))
            .cloned()
            .collect();
        for rule in matches {
            let code = self.invoke(&rule, event)?;
            if code != 0 {
                return Ok(code);
            }
        }
        Ok(0)
    }

    pub fn named(&mut self, name: &str, event: &Event) -> Result<u8> {
        let rule = self
            .rules
            .rules
            .iter()
            .find(|rule| rule.name == name)
            .cloned()
            .ok_or_else(|| input(format!("Unknown rule: {name}")))?;
        match rule.matches(event) {
            true => self.invoke(&rule, event),
            false => Ok(0),
        }
    }

    fn invoke(&mut self, rule: &crate::rules::Rule, event: &Event) -> Result<u8> {
        if self.active.contains(&rule.name) {
            return Err(input(format!(
                "Rule cycle: {} -> {}",
                self.active.join(" -> "),
                rule.name
            )));
        }
        eprintln!(
            "rule {}: {} -> {}",
            rule.name, event.mode, rule.action.profile
        );
        let mut profile = load(&self.root, &rule.action.profile)?;
        profile.env.extend(event.environment(&rule.name)?);
        self.active.push(rule.name.clone());
        let result = self.profile(profile, None, None, None);
        self.active.pop();
        result
    }

    pub fn profile(
        &mut self,
        profile: Profile,
        cwd: Option<PathBuf>,
        repeat: Option<u64>,
        interval: Option<u64>,
    ) -> Result<u8> {
        let cwd = absolute(cwd.unwrap_or(profile.cwd.clone()))?;
        let repeat = repeat.unwrap_or(profile.repeat);
        for iteration in 1..=repeat {
            eprintln!("run {iteration}/{repeat}: {}", profile.name);
            for (index, step) in profile.steps.iter().enumerate() {
                sleep(Duration::from_millis(step.delay_ms));
                eprintln!("step {}: {}", index + 1, step.name);
                let (program, args) = match &step.action {
                    StepAction::Exec { program, args } => (program.clone(), args.clone()),
                    StepAction::Shell { command } => {
                        ("bash".into(), vec!["-c".into(), command.clone()])
                    }
                };
                let event = Event {
                    mode: Mode::Before,
                    command: Some(program),
                    args,
                    profile: Some(profile.name.clone()),
                    step: Some(step.name.clone()),
                    cwd: absolute(
                        step.cwd
                            .as_ref()
                            .map_or_else(|| cwd.clone(), |path| cwd.join(path)),
                    )?,
                    paths: Vec::new(),
                    exit_code: None,
                };
                let mut env = profile.env.clone();
                env.extend(step.env.clone());
                let code = self.command(event, env)?;
                if code != 0 {
                    return Ok(code);
                }
            }
            if iteration < repeat {
                sleep(Duration::from_millis(
                    interval.unwrap_or(profile.interval_ms),
                ));
            }
        }
        Ok(0)
    }

    pub fn external(&mut self, program: &str, args: &[String], cwd: PathBuf) -> Result<u8> {
        self.command(
            Event {
                mode: Mode::Before,
                command: Some(program.into()),
                args: args.to_vec(),
                profile: None,
                step: None,
                cwd,
                paths: Vec::new(),
                exit_code: None,
            },
            BTreeMap::new(),
        )
    }

    fn command(&mut self, mut event: Event, env: BTreeMap<String, String>) -> Result<u8> {
        let before = self.emit(&event)?;
        if before != 0 {
            return Ok(before);
        }
        let result = Command::new(event.command.as_ref().unwrap())
            .args(&event.args)
            .current_dir(&event.cwd)
            .envs(env)
            .status();
        let code = match &result {
            Ok(status) => exit_code(*status),
            Err(_) => 1,
        };
        event.mode = Mode::After;
        event.exit_code = Some(code);
        let after = self.emit(&event)?;
        if code != 0 {
            event.mode = Mode::Error;
            self.emit(&event)?;
        }
        result?;
        Ok(if code != 0 { code } else { after })
    }
}

fn exit_code(status: ExitStatus) -> u8 {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(unix)]
    let code = status.code().unwrap_or(128 + status.signal().unwrap_or(1));
    #[cfg(not(unix))]
    let code = status.code().unwrap_or(1);
    u8::try_from(code).unwrap_or(1)
}
