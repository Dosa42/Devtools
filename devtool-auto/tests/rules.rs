use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn cli(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_devtool-auto"));
    command
        .arg("--profiles-dir")
        .arg(root.join("profiles"))
        .args(args)
        .current_dir(root);
    command
}

fn ok(root: &Path, args: &[&str]) -> Output {
    let output = cli(root, args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn profile(root: &Path, name: &str, script: &str) {
    ok(root, &["profile", "create", name]);
    ok(
        root,
        &["step", "add", name, "--name", "work", "--command", script],
    );
}

fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !condition() {
        assert!(Instant::now() < deadline);
        sleep(Duration::from_millis(10));
    }
}

#[test]
fn own_language_canonical_json_and_text_roundtrip() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let source = "rule archive-after-tree\n  mode after\n  matcher command devtool\n  matcher flag canonical-tree\n  action profile archive\nend\n\nrule hourly-backup\n  mode timer\n  interval 1h\n  after 2s\n  runs 3\n  action profile backup\nend\n";
    fs::write(root.join("flow.rules"), source).unwrap();
    ok(root, &["rule", "check", "flow.rules"]);
    ok(root, &["rule", "import", "flow.rules"]);
    ok(
        root,
        &["rule", "export", "--format", "json", "-o", "canonical.json"],
    );
    let canonical: Value =
        serde_json::from_slice(&fs::read(root.join("canonical.json")).unwrap()).unwrap();
    assert_eq!(canonical["schema"], "devtool-auto.rules/1");
    assert_eq!(canonical["rules"][1]["timer"]["interval_ms"], 3_600_000);
    ok(
        root,
        &["rule", "export", "--format", "text", "-o", "export.rules"],
    );
    ok(root, &["rule", "import", "export.rules"]);
    let after: Value =
        serde_json::from_slice(&ok(root, &["rule", "export", "--format", "json"]).stdout).unwrap();
    assert_eq!(canonical, after);
    ok(root, &["rule", "import", "canonical.json"]);
    let names: Value = serde_json::from_slice(&ok(root, &["profile", "list"]).stdout).unwrap();
    assert_eq!(names, serde_json::json!([]));
    let schema: Value = serde_json::from_slice(&ok(root, &["rule", "schema"]).stdout).unwrap();
    assert_eq!(
        schema["properties"]["schema"]["const"],
        "devtool-auto.rules/1"
    );
}

#[test]
fn command_flags_and_all_matchers_are_observed_without_execution() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    ok(
        root,
        &[
            "rule",
            "set",
            "tree",
            "--mode",
            "after",
            "--action",
            "archive",
            "--matcher",
            "command devtool",
            "--matcher",
            "flag canonical-tree",
            "--matcher",
            "profile build-*",
            "--matcher",
            "step templates",
            "--matcher",
            "exit 0",
        ],
    );
    let matched: Value = serde_json::from_slice(
        &ok(
            root,
            &[
                "rule",
                "match",
                "--mode",
                "after",
                "--command",
                "/usr/bin/devtool",
                "--arg=--canonical-tree",
                "--profile",
                "build-one",
                "--step",
                "templates",
                "--exit-code",
                "0",
            ],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(matched, serde_json::json!(["tree"]));
    let missing: Value = serde_json::from_slice(
        &ok(
            root,
            &[
                "rule",
                "match",
                "--mode",
                "after",
                "--command",
                "devtool",
                "--arg=--other",
                "--profile",
                "build-one",
                "--step",
                "templates",
                "--exit-code",
                "0",
            ],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(missing, serde_json::json!([]));
    assert!(!root.join("profiles/archive.json").exists());
}

#[test]
fn profile_steps_emit_before_and_after_in_order_with_event_context() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "main", "printf c >> trace");
    profile(root, "pre", "printf b >> trace");
    profile(
        root,
        "post",
        "printf a >> trace; printf '%s' \"$DEVTOOL_AUTO_EVENT\" > event.json",
    );
    for (name, mode, action) in [
        ("before-main", "before", "pre"),
        ("after-main", "after", "post"),
    ] {
        ok(
            root,
            &[
                "rule",
                "set",
                name,
                "--mode",
                mode,
                "--action",
                action,
                "--matcher",
                "profile main",
                "--matcher",
                "step work",
            ],
        );
    }
    ok(root, &["run", "main"]);
    assert_eq!(fs::read_to_string(root.join("trace")).unwrap(), "bca");
    let event: Value = serde_json::from_slice(&fs::read(root.join("event.json")).unwrap()).unwrap();
    assert_eq!(event["mode"], "after");
    assert_eq!(event["command"], "bash");
    assert_eq!(event["profile"], "main");
    assert_eq!(event["exit_code"], 0);
}

#[test]
fn external_wrapper_failure_runs_after_and_error_and_preserves_status() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    for (name, script) in [
        ("pre", "printf b >> trace"),
        ("post", "printf a >> trace"),
        ("failed", "printf e >> trace"),
    ] {
        profile(root, name, script);
    }
    for (name, mode, action) in [
        ("one", "before", "pre"),
        ("two", "after", "post"),
        ("three", "error", "failed"),
    ] {
        ok(
            root,
            &[
                "rule",
                "set",
                name,
                "--mode",
                mode,
                "--action",
                action,
                "--matcher",
                "command sh",
            ],
        );
    }
    let output = cli(
        root,
        &["exec", "--", "sh", "-c", "printf c >> trace; exit 7"],
    )
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(fs::read_to_string(root.join("trace")).unwrap(), "bcae");
}

#[test]
fn saved_timer_rule_runs_only_its_own_action() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "ticks", "printf x >> ticks");
    profile(root, "other", "touch other");
    ok(
        root,
        &[
            "rule",
            "set",
            "clock",
            "--mode",
            "timer",
            "--action",
            "ticks",
            "--interval",
            "10ms",
            "--after",
            "20ms",
            "--runs",
            "2",
        ],
    );
    ok(
        root,
        &[
            "rule",
            "set",
            "unstarted",
            "--mode",
            "timer",
            "--action",
            "other",
            "--interval",
            "1h",
        ],
    );
    let start = Instant::now();
    ok(root, &["rule", "start", "clock"]);
    assert!(start.elapsed() >= Duration::from_millis(30));
    assert_eq!(fs::read_to_string(root.join("ticks")).unwrap(), "xx");
    assert!(!root.join("other").exists());
}

#[test]
fn saved_change_rule_receives_changed_paths_and_debounces() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("inbox/deep")).unwrap();
    profile(
        root,
        "process",
        "printf '%s' \"$DEVTOOL_AUTO_EVENT\" > changed.json",
    );
    fs::write(root.join("watch.rules"), "rule incoming-files\n mode change\n matcher path inbox\n recursive true\n poll 10ms\n debounce 50ms\n runs 1\n action profile process\nend\n").unwrap();
    ok(root, &["rule", "import", "watch.rules"]);
    let log = root.join("watch.log");
    let mut child = cli(root, &["rule", "start", "incoming-files"])
        .stdout(Stdio::null())
        .stderr(fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(|| {
        until(|| fs::read_to_string(&log).unwrap().contains("watch ready"));
        fs::write(root.join("inbox/deep/file.txt"), "one").unwrap();
        sleep(Duration::from_millis(20));
        fs::write(root.join("inbox/deep/file.txt"), "two").unwrap();
        until(|| root.join("changed.json").exists());
    });
    if result.is_err() {
        child.kill().unwrap();
    }
    assert!(child.wait().unwrap().success());
    result.unwrap();
    let event: Value =
        serde_json::from_slice(&fs::read(root.join("changed.json")).unwrap()).unwrap();
    assert_eq!(event["mode"], "change");
    assert!(
        event["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str().unwrap().ends_with("deep/file.txt"))
    );
}

#[test]
fn rule_enable_disable_emit_and_remove() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "action", "printf x >> activated");
    ok(
        root,
        &[
            "rule",
            "set",
            "manual",
            "--mode",
            "after",
            "--action",
            "action",
            "--matcher",
            "command external",
        ],
    );
    ok(root, &["rule", "disable", "manual"]);
    ok(
        root,
        &["rule", "emit", "--mode", "after", "--command", "external"],
    );
    assert!(!root.join("activated").exists());
    ok(root, &["rule", "enable", "manual"]);
    ok(
        root,
        &["rule", "emit", "--mode", "after", "--command", "external"],
    );
    assert_eq!(fs::read_to_string(root.join("activated")).unwrap(), "x");
    ok(root, &["rule", "remove", "manual"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&ok(root, &["rule", "list"]).stdout).unwrap(),
        serde_json::json!([])
    );
}

#[test]
fn invalid_language_and_json_are_rejected_without_changing_rules() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    ok(
        root,
        &[
            "rule", "set", "keep", "--mode", "after", "--action", "action",
        ],
    );
    let original = fs::read(root.join("profiles/rules/rules.json")).unwrap();
    for source in [
        "rule bad\nmode after\naction profile action\n",
        "rule bad\nmode unknown\naction profile action\nend",
        "rule bad\nmode timer\ninterval 0ms\naction profile action\nend",
        "rule bad\nmode change\naction profile action\nend",
        "rule bad\nmode after\nmode before\naction profile action\nend",
        "rule bad\nmode after\ninterval 1h\naction profile action\nend",
        "rule bad\nmode after\nmatcher secret value\naction profile action\nend",
    ] {
        fs::write(root.join("bad.rules"), source).unwrap();
        assert!(
            !cli(root, &["rule", "import", "bad.rules"])
                .output()
                .unwrap()
                .status
                .success()
        );
        assert_eq!(
            fs::read(root.join("profiles/rules/rules.json")).unwrap(),
            original
        );
    }
    let mut invalid: Value = serde_json::from_slice(&original).unwrap();
    invalid["rules"][0]["unexpected"] = true.into();
    fs::write(root.join("bad.json"), serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(
        !cli(root, &["rule", "import", "bad.json"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn action_cycles_report_rule_chain() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    profile(root, "main", "printf x");
    ok(
        root,
        &[
            "rule",
            "set",
            "loop",
            "--mode",
            "before",
            "--action",
            "main",
            "--matcher",
            "profile main",
        ],
    );
    let output = cli(root, &["run", "main"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Rule cycle: loop -> loop"));
}
