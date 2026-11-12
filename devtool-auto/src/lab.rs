use super::{Profile, Result, Step, StepAction, print_json, save};
use crate::triggers::{Timer, Watch};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn step(name: &str, action: StepAction) -> Step {
    Step {
        name: name.to_owned(),
        cwd: None,
        env: BTreeMap::new(),
        delay_ms: 0,
        action,
    }
}

fn exec(name: &str, arguments: &[&str]) -> Step {
    step(
        name,
        StepAction::Exec {
            program: "devtool".into(),
            args: arguments.iter().map(|value| (*value).to_owned()).collect(),
        },
    )
}

fn shell(name: &str, command: &str) -> Step {
    step(
        name,
        StepAction::Shell {
            command: command.into(),
        },
    )
}

pub fn init(profiles: &Path, path: PathBuf) -> Result<()> {
    for directory in [
        "bin",
        "sbin",
        "etc",
        "home/demo/projects",
        "usr/bin",
        "usr/lib",
        "usr/share/demo",
        "var/log",
        "var/backups",
        "var/lib/state",
        "tmp/incoming",
        "opt",
        "dev",
        "proc",
        "sys",
        "run",
        "mnt",
        "media",
        "root",
    ] {
        fs::create_dir_all(path.join(directory))?;
    }
    for (file, contents) in [
        ("etc/hostname", "devtool-lab\n"),
        ("etc/os-release", "NAME=DevtoolLab\nVERSION=0.1.0\n"),
        ("etc/demo.json", "{\"environment\":\"fake-linux-tree\"}\n"),
        ("home/demo/welcome.txt", "Devtool automation lab\n"),
        ("usr/share/demo/input.txt", "Automation test input\n"),
        ("var/log/lab.log", ""),
    ] {
        fs::write(path.join(file), contents)?;
    }
    let env = BTreeMap::from([
        ("HOME".into(), path.join("home/demo").display().to_string()),
        (
            "XDG_CONFIG_HOME".into(),
            path.join("home/demo/.config").display().to_string(),
        ),
        (
            "XDG_STATE_HOME".into(),
            path.join("var/lib/state").display().to_string(),
        ),
    ]);
    let base = |name: &str, steps| Profile {
        schema_version: 1,
        name: name.into(),
        cwd: path.clone(),
        repeat: 1,
        interval_ms: 0,
        env: env.clone(),
        steps,
        timer: None,
        watch: None,
    };
    let build = base(
        "lab-build",
        vec![
            exec("hoofdmap", &["-D", "--path", "home/demo/projects"]),
            exec("canonical-tree", &["--canonical-tree"]),
            shell(
                "bestandenlijst",
                "find home/demo/projects -type f | sort > var/log/files.txt",
            ),
            exec(
                "7z",
                &[
                    "compress",
                    "home/demo/projects",
                    "--format",
                    "7z",
                    "-o",
                    "var/backups/projects.7z",
                ],
            ),
            exec(
                "tar.gz",
                &[
                    "compress",
                    "home/demo/projects",
                    "--format",
                    "tar.gz",
                    "-o",
                    "var/backups/projects.tar.gz",
                ],
            ),
        ],
    );
    let mut timer = base(
        "lab-timer",
        vec![shell(
            "timer-log",
            "date '+%Y-%m-%dT%H:%M:%S%z' >> var/log/timer.log",
        )],
    );
    timer.timer = Some(Timer {
        after_ms: 500,
        interval_ms: 1000,
        runs: 3,
    });
    let mut watch = base(
        "lab-watch",
        vec![shell(
            "watch-log",
            "date '+%Y-%m-%dT%H:%M:%S%z' >> var/log/watch.log; find tmp/incoming -type f | sort >> var/log/watch.log",
        )],
    );
    watch.watch = Some(Watch {
        paths: vec!["tmp/incoming".into()],
        recursive: true,
        poll_ms: 100,
        debounce_ms: 250,
        initial: false,
        runs: 0,
    });
    for profile in [build, timer, watch] {
        save(profiles, &profile, true)?;
    }
    print_json(
        &serde_json::json!({"path": path, "profiles": ["lab-build", "lab-timer", "lab-watch"]}),
    )
}
