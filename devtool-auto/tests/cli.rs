use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_devtool-auto"));
    command
        .arg("--profiles-dir")
        .arg(root.join("profiles"))
        .args(args)
        .current_dir(root);
    command
}

fn ok(root: &Path, args: &[&str]) -> Output {
    let output = command(root, args).output().unwrap();
    assert!(
        output.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn profile(root: &Path, name: &str) {
    ok(root, &["profile", "create", name]);
}

fn shell(root: &Path, name: &str, script: &str) {
    ok(
        root,
        &["step", "add", name, "--name", "shell", "--command", script],
    );
}

fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !condition() {
        assert!(Instant::now() < deadline, "condition timed out");
        sleep(Duration::from_millis(10));
    }
}

#[test]
fn profile_roundtrip_settings_and_schema() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "flow");
    ok(
        root,
        &[
            "profile",
            "set",
            "flow",
            "--repeat",
            "2",
            "--interval-ms",
            "3",
            "--env",
            "VALUE=a=b",
        ],
    );
    shell(root, "flow", "printf '%s' \"$VALUE\" >> output.txt");
    ok(root, &["run", "flow"]);
    assert_eq!(
        fs::read_to_string(root.join("output.txt")).unwrap(),
        "a=ba=b"
    );
    ok(root, &["profile", "export", "flow", "-o", "export.json"]);
    ok(
        root,
        &["profile", "import", "export.json", "--name", "copy"],
    );
    let profile: Value =
        serde_json::from_slice(&ok(root, &["profile", "show", "copy"]).stdout).unwrap();
    assert_eq!(profile["repeat"], 2);
    assert_eq!(profile["steps"][0]["action"]["kind"], "shell");
    let schema: Value = serde_json::from_slice(&ok(root, &["schema"]).stdout).unwrap();
    assert_eq!(schema["properties"]["schema_version"]["const"], 1);
    ok(root, &["profile", "remove", "copy"]);
}

#[test]
fn exec_preserves_flags_spaces_and_step_environment() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "exec");
    ok(
        root,
        &[
            "step",
            "add",
            "exec",
            "--name",
            "literal",
            "--program",
            "bash",
            "--arg=-c",
            "--arg=printf '%s|%s' \"$1\" \"$SETTING\" > result",
            "--arg=ignored",
            "--arg=--literal with space",
            "--env",
            "SETTING=step",
        ],
    );
    ok(root, &["run", "exec"]);
    assert_eq!(
        fs::read_to_string(root.join("result")).unwrap(),
        "--literal with space|step"
    );
}

#[test]
fn cwd_overrides_step_directory_and_delay() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("other/sub")).unwrap();
    profile(root, "cwd");
    ok(
        root,
        &[
            "step",
            "add",
            "cwd",
            "--name",
            "inside",
            "--command",
            "pwd > location",
            "--cwd",
            "sub",
            "--delay-ms",
            "40",
        ],
    );
    let start = Instant::now();
    ok(
        root,
        &[
            "run",
            "cwd",
            "--cwd",
            "other",
            "--repeat",
            "2",
            "--interval-ms",
            "30",
        ],
    );
    assert!(start.elapsed() >= Duration::from_millis(110));
    assert_eq!(
        fs::read_to_string(root.join("other/sub/location"))
            .unwrap()
            .trim(),
        root.join("other/sub").to_str().unwrap()
    );
}

#[test]
fn failure_returns_child_status_and_steps_can_be_removed() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "error");
    shell(root, "error", "exit 7");
    shell(root, "error", "touch next");
    let output = command(root, &["run", "error"]).output().unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert!(!root.join("next").exists());
    ok(root, &["step", "remove", "error", "--index", "1"]);
    ok(root, &["run", "error"]);
    assert!(root.join("next").exists());
    ok(root, &["step", "clear", "error"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&ok(root, &["step", "list", "error"]).stdout).unwrap(),
        serde_json::json!([])
    );
}

#[test]
fn saved_timer_and_temporary_overrides() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "clock");
    shell(root, "clock", "printf x >> ticks");
    ok(
        root,
        &[
            "timer",
            "set",
            "clock",
            "--after-ms",
            "40",
            "--interval-ms",
            "30",
            "--runs",
            "3",
        ],
    );
    let start = Instant::now();
    ok(root, &["timer", "start", "clock"]);
    assert!(start.elapsed() >= Duration::from_millis(100));
    assert_eq!(fs::read_to_string(root.join("ticks")).unwrap(), "xxx");
    ok(
        root,
        &["timer", "start", "clock", "--after-ms", "0", "--runs", "1"],
    );
    let settings: Value =
        serde_json::from_slice(&ok(root, &["timer", "show", "clock"]).stdout).unwrap();
    assert_eq!(settings["runs"], 3);
    ok(root, &["timer", "clear", "clock"]);
}

#[test]
fn recursive_watcher_detects_create_modify_delete_and_debounces() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("inbox/nested")).unwrap();
    profile(root, "events");
    shell(root, "events", "printf x >> events");
    ok(
        root,
        &[
            "watch",
            "set",
            "events",
            "--path",
            "inbox",
            "--recursive",
            "true",
            "--poll-ms",
            "10",
            "--debounce-ms",
            "50",
            "--runs",
            "3",
        ],
    );
    let log = root.join("watch.log");
    let mut child = command(root, &["watch", "start", "events"])
        .stdout(Stdio::null())
        .stderr(fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(|| {
        until(|| fs::read_to_string(&log).unwrap().contains("watch ready"));
        let file = root.join("inbox/nested/input");
        fs::write(&file, "one").unwrap();
        sleep(Duration::from_millis(20));
        fs::write(&file, "two").unwrap();
        until(|| fs::read_to_string(root.join("events")).unwrap_or_default() == "x");
        fs::write(&file, "longer input").unwrap();
        until(|| fs::read_to_string(root.join("events")).unwrap_or_default() == "xx");
        fs::remove_file(file).unwrap();
        until(|| fs::read_to_string(root.join("events")).unwrap_or_default() == "xxx");
    });
    if result.is_err() {
        child.kill().unwrap();
    }
    assert!(child.wait().unwrap().success());
    result.unwrap();
    ok(root, &["watch", "clear", "events"]);
}

#[test]
fn watcher_initial_override_is_one_run() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "initial");
    shell(root, "initial", "touch initial");
    ok(
        root,
        &[
            "watch",
            "set",
            "initial",
            "--path",
            "missing",
            "--initial",
            "false",
        ],
    );
    ok(
        root,
        &[
            "watch",
            "start",
            "initial",
            "--initial",
            "true",
            "--runs",
            "1",
        ],
    );
    assert!(root.join("initial").is_file());
}

#[test]
fn lab_init_creates_fake_tree_and_saved_examples() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    ok(root, &["lab", "init", "--path", "linux-fs"]);
    for path in [
        "bin", "etc", "usr", "var", "home", "tmp", "proc", "dev", "sys",
    ] {
        assert!(root.join("linux-fs").join(path).is_dir());
    }
    assert_eq!(
        fs::read_to_string(root.join("linux-fs/etc/hostname")).unwrap(),
        "devtool-lab\n"
    );
    let build: Value =
        serde_json::from_slice(&ok(root, &["profile", "show", "lab-build"]).stdout).unwrap();
    assert_eq!(build["steps"].as_array().unwrap().len(), 5);
    assert_eq!(build["cwd"], root.join("linux-fs").to_str().unwrap());
    ok(
        root,
        &[
            "timer",
            "start",
            "lab-timer",
            "--after-ms",
            "0",
            "--interval-ms",
            "1",
            "--runs",
            "1",
        ],
    );
    assert!(root.join("linux-fs/var/log/timer.log").is_file());
    ok(
        root,
        &[
            "watch",
            "start",
            "lab-watch",
            "--initial",
            "true",
            "--runs",
            "1",
        ],
    );
    assert!(root.join("linux-fs/var/log/watch.log").is_file());
}
