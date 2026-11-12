use super::{Profile, Result, input, load, print_json, run, save};
use crate::cli::{TimerCommand, WatchCommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timer {
    pub after_ms: u64,
    pub interval_ms: u64,
    pub runs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watch {
    pub paths: Vec<PathBuf>,
    pub recursive: bool,
    pub poll_ms: u64,
    pub debounce_ms: u64,
    pub initial: bool,
    pub runs: u64,
}

fn activate(root: &Path, profile: &Profile, kind: &str, count: u64) -> Result<u8> {
    eprintln!("{kind} activation {count}: {}", profile.name);
    run(root, profile.clone(), None, None, None)
}

pub fn timer(root: &Path, command: TimerCommand) -> Result<u8> {
    match command {
        TimerCommand::Set {
            profile,
            after_ms,
            interval_ms,
            runs,
        } => {
            let mut profile = load(root, &profile)?;
            profile.timer = Some(Timer {
                after_ms,
                interval_ms,
                runs,
            });
            save(root, &profile, false)?;
        }
        TimerCommand::Show { profile } => print_json(&load(root, &profile)?.timer)?,
        TimerCommand::Clear { profile } => {
            let mut profile = load(root, &profile)?;
            profile.timer = None;
            save(root, &profile, false)?;
        }
        TimerCommand::Start {
            profile,
            after_ms,
            interval_ms,
            runs,
        } => {
            let profile = load(root, &profile)?;
            let settings = profile
                .timer
                .as_ref()
                .ok_or_else(|| input("Configure a timer with timer set first"))?;
            let interval = interval_ms.unwrap_or(settings.interval_ms);
            if interval == 0 {
                return Err(input("Timer interval_ms must be >= 1"));
            }
            let limit = runs.unwrap_or(settings.runs);
            sleep(Duration::from_millis(after_ms.unwrap_or(settings.after_ms)));
            let mut count = 0;
            loop {
                count += 1;
                let code = activate(root, &profile, "timer", count)?;
                if code != 0 {
                    return Ok(code);
                }
                if limit != 0 && count >= limit {
                    break;
                }
                sleep(Duration::from_millis(interval));
            }
        }
    }
    Ok(0)
}

#[derive(PartialEq)]
struct Stamp {
    modified: Option<SystemTime>,
    bytes: u64,
    kind: u8,
}

type Snapshot = BTreeMap<PathBuf, Stamp>;

fn scan(path: &Path, recursive: bool, children: bool, result: &mut Snapshot) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let directory = metadata.is_dir();
    result.insert(
        path.to_owned(),
        Stamp {
            modified: metadata.modified().ok(),
            bytes: metadata.len(),
            kind: if directory {
                1
            } else if metadata.is_symlink() {
                2
            } else {
                0
            },
        },
    );
    if directory && children {
        for child in fs::read_dir(path)? {
            scan(&child?.path(), recursive, recursive, result)?;
        }
    }
    Ok(())
}

fn snapshot(cwd: &Path, settings: &Watch) -> Result<Snapshot> {
    let mut result = BTreeMap::new();
    for path in &settings.paths {
        scan(&cwd.join(path), settings.recursive, true, &mut result)?;
    }
    Ok(result)
}

pub(crate) fn watch_events(
    cwd: &Path,
    settings: Watch,
    mut callback: impl FnMut(Vec<PathBuf>) -> Result<u8>,
) -> Result<u8> {
    if settings.poll_ms == 0 || settings.paths.is_empty() {
        return Err(input("Watcher requires paths and poll_ms >= 1"));
    }
    let mut previous = snapshot(cwd, &settings)?;
    let mut count = 0;
    let mut changed = None;
    let mut paths = std::collections::BTreeSet::new();
    if settings.initial {
        count += 1;
        let code = callback(settings.paths.iter().map(|path| cwd.join(path)).collect())?;
        if code != 0 || settings.runs == count {
            return Ok(code);
        }
        previous = snapshot(cwd, &settings)?;
    }
    eprintln!("watch ready: {}", cwd.display());
    loop {
        sleep(Duration::from_millis(settings.poll_ms));
        let current = snapshot(cwd, &settings)?;
        if current != previous {
            for path in current.keys().chain(previous.keys()) {
                if current.get(path) != previous.get(path) {
                    paths.insert(path.clone());
                }
            }
            changed = Some(Instant::now());
            previous = current;
        }
        if changed.is_some_and(|at| at.elapsed() >= Duration::from_millis(settings.debounce_ms)) {
            count += 1;
            let code = callback(paths.iter().cloned().collect())?;
            if code != 0 || settings.runs == count {
                return Ok(code);
            }
            paths.clear();
            changed = None;
        }
    }
}

fn watch_loop(root: &Path, profile: Profile, settings: Watch) -> Result<u8> {
    let mut count = 0;
    watch_events(&profile.cwd, settings, |_| {
        count += 1;
        activate(root, &profile, "watch", count)
    })
}

pub fn watch(root: &Path, command: WatchCommand) -> Result<u8> {
    match command {
        WatchCommand::Set {
            profile,
            paths,
            recursive,
            poll_ms,
            debounce_ms,
            initial,
            runs,
        } => {
            let mut profile = load(root, &profile)?;
            profile.watch = Some(Watch {
                paths,
                recursive,
                poll_ms,
                debounce_ms,
                initial,
                runs,
            });
            save(root, &profile, false)?;
        }
        WatchCommand::Show { profile } => print_json(&load(root, &profile)?.watch)?,
        WatchCommand::Clear { profile } => {
            let mut profile = load(root, &profile)?;
            profile.watch = None;
            save(root, &profile, false)?;
        }
        WatchCommand::Start {
            profile,
            paths,
            recursive,
            poll_ms,
            debounce_ms,
            initial,
            runs,
        } => {
            let profile = load(root, &profile)?;
            let mut settings = profile
                .watch
                .clone()
                .ok_or_else(|| input("Configure a watcher with watch set first"))?;
            if !paths.is_empty() {
                settings.paths = paths;
            }
            if let Some(value) = recursive {
                settings.recursive = value;
            }
            if let Some(value) = poll_ms {
                settings.poll_ms = value;
            }
            if let Some(value) = debounce_ms {
                settings.debounce_ms = value;
            }
            if let Some(value) = initial {
                settings.initial = value;
            }
            if let Some(value) = runs {
                settings.runs = value;
            }
            return watch_loop(root, profile, settings);
        }
    }
    Ok(0)
}
