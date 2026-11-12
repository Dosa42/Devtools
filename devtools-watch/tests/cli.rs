use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread::sleep;
use std::time::{Duration, Instant};
use tempfile::TempDir;

struct Lab {
    temp: TempDir,
}

impl Lab {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }
    fn path(&self) -> &Path {
        self.temp.path()
    }
    fn cli(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_devtools-watch"));
        command
            .args(args)
            .current_dir(self.path())
            .env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.path().join("config"))
            .env("XDG_STATE_HOME", self.path().join("state"));
        command
    }
    fn output(&self, args: &[&str]) -> Output {
        self.cli(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let output = self.output(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn configure(&self, target: Option<Value>, parents: Vec<PathBuf>) {
        let proof = std::env::var_os("DEVTOOLS_WATCH_TEST_PROOF")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("devtool-proof"));
        fs::write(self.path().join("settings.json"), serde_json::to_vec(&json!({"schema":"devtools-watch.config/1","parents":parents,"targets":target.into_iter().collect::<Vec<_>>(),"proof_executable":proof,"poll_ms":30,"verify_interval_ms":100})).unwrap()).unwrap();
        self.ok(&["--config", "settings.json"]);
    }
    fn proof(&self, args: &[&str]) {
        let executable =
            std::env::var_os("DEVTOOLS_WATCH_TEST_PROOF").unwrap_or_else(|| "devtool-proof".into());
        let output = Command::new(executable)
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = self.cli(&["--off"]).output();
    }
}

fn until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(Instant::now() < deadline, "timeout");
        sleep(Duration::from_millis(20));
    }
}

fn fixture(root: &Path) {
    fs::create_dir(root).unwrap();
    let tree: Value = serde_json::from_str(devtools_watch::state::TREE).unwrap();
    for directory in tree["directories"].as_array().unwrap() {
        fs::create_dir(root.join(directory.as_str().unwrap())).unwrap();
    }
    for entry in tree["files"].as_array().unwrap() {
        fs::write(root.join(entry["path"].as_str().unwrap()), b"fixture").unwrap();
    }
    fs::write(root.join("manifest.json"), br#"{"schema_version":1,"template":"canonical-tree","template_version":2,"derived_outputs":["state.txt"]}"#).unwrap();
    fs::write(root.join("state.txt"), devtools_watch::state::TEMPLATE).unwrap();
    fs::write(root.join("checksums.sha"), []).unwrap();
}

fn state(root: &Path, key: &str, value: bool) -> bool {
    fs::read_to_string(root.join("state.txt"))
        .is_ok_and(|text| text.lines().any(|line| line == format!("{key}: {value}")))
}

#[test]
fn template_protocol_and_exact_three_actions() {
    assert_eq!(
        devtools_watch::state::State::default().text(),
        devtools_watch::state::TEMPLATE
    );
    let lab = Lab::new();
    assert_eq!(
        lab.ok(&["--config"])["actions"],
        json!(["--on", "--off", "--config"])
    );
    for args in [
        vec![],
        vec!["--on", "--off"],
        vec!["--status"],
        vec!["--help"],
        vec!["--version"],
    ] {
        assert_eq!(lab.output(&args).status.code(), Some(2));
    }
}

#[test]
fn background_single_instance_and_only_state_is_modified() {
    let lab = Lab::new();
    let root = lab.path().join("20261112T120000Z_run-nr.001");
    fixture(&root);
    lab.configure(None, vec![lab.path().to_owned()]);
    let before = fs::read(root.join("document.pdf")).unwrap();
    let original = fs::metadata(root.join("document.pdf"))
        .unwrap()
        .modified()
        .unwrap();
    let first = lab.ok(&["--on"]);
    let second = lab.ok(&["--on"]);
    assert_eq!(first["pid"], second["pid"]);
    until(|| state(&root, "canonical_tree", true));
    assert!(state(&root, "sha_checksum", false));
    assert_eq!(fs::read(root.join("document.pdf")).unwrap(), before);
    assert_eq!(
        fs::metadata(root.join("document.pdf"))
            .unwrap()
            .modified()
            .unwrap(),
        original
    );
    let content = fs::read_to_string(root.join("state.txt")).unwrap();
    assert!(
        content
            .lines()
            .skip(1)
            .all(|line| line.ends_with(": true") || line.ends_with(": false"))
    );
    sleep(Duration::from_millis(200));
    let stamp = fs::metadata(root.join("state.txt"))
        .unwrap()
        .modified()
        .unwrap();
    sleep(Duration::from_millis(250));
    assert_eq!(
        fs::metadata(root.join("state.txt"))
            .unwrap()
            .modified()
            .unwrap(),
        stamp
    );
    lab.ok(&["--off"]);
    assert_eq!(lab.ok(&["--config"])["running"], false);
    fs::write(root.join("state.txt"), "external").unwrap();
    sleep(Duration::from_millis(150));
    assert_eq!(
        fs::read_to_string(root.join("state.txt")).unwrap(),
        "external"
    );
}

#[test]
fn observer_starts_before_tree_and_config_reloads_live() {
    let lab = Lab::new();
    lab.configure(None, vec![lab.path().to_owned()]);
    lab.ok(&["--on"]);
    let root = lab.path().join("20261112T120000Z_run-nr.002");
    fs::create_dir(&root).unwrap();
    sleep(Duration::from_millis(100));
    assert!(!root.join("state.txt").exists());
    fs::remove_dir(&root).unwrap();
    fixture(&root);
    until(|| state(&root, "canonical_tree", true));
    let second = lab.path().join("explicit-root");
    fixture(&second);
    lab.configure(Some(json!({"root":second})), vec![]);
    until(|| state(&second, "canonical_tree", true));
    sleep(Duration::from_millis(100));
    fs::write(root.join("state.txt"), "unchanged").unwrap();
    sleep(Duration::from_millis(150));
    assert_eq!(
        fs::read_to_string(root.join("state.txt")).unwrap(),
        "unchanged"
    );
}

#[test]
fn real_checksums_provenance_signatures_and_tampering() {
    let lab = Lab::new();
    let root = lab.path().join("root");
    fixture(&root);
    let root_text = root.to_str().unwrap();
    lab.proof(&["manifest", "create", root_text, "--derived", "state.txt"]);
    lab.proof(&[
        "provenance",
        "create",
        root_text,
        "--derived",
        "state.txt",
        "-o",
        "provenance.json",
    ]);
    lab.proof(&[
        "key",
        "generate",
        "--private",
        "private.json",
        "--public",
        "public.json",
    ]);
    lab.proof(&[
        "sign",
        root.join("checksums.sha").to_str().unwrap(),
        "--key",
        "private.json",
        "-o",
        "checksum.sig",
    ]);
    lab.proof(&["sign", "provenance.json", "--key", "private.json"]);
    lab.configure(Some(json!({"root":root,"checksum_signature":lab.path().join("checksum.sig"),"provenance":lab.path().join("provenance.json"),"provenance_signature":lab.path().join("provenance.json.sig"),"public_key":lab.path().join("public.json")})),vec![]);
    lab.ok(&["--on"]);
    until(|| {
        state(&root, "sha_checksum", true)
            && state(&root, "provenance", true)
            && state(&root, "sha_signature", true)
            && state(&root, "provenance_signature", true)
    });
    let expected = fs::read(root.join("state.txt")).unwrap();
    assert!(
        String::from_utf8_lossy(&expected)
            .lines()
            .skip(1)
            .all(|line| line.ends_with(": true"))
    );
    fs::write(root.join("document.pdf"), "changed").unwrap();
    until(|| state(&root, "sha_checksum", false) && state(&root, "provenance", false));
    assert!(state(&root, "sha_signature", true));
    fs::write(root.join("document.pdf"), "fixture").unwrap();
    until(|| state(&root, "sha_checksum", true) && state(&root, "provenance", true));
    fs::write(root.join("state.txt"), "external").unwrap();
    until(|| fs::read(root.join("state.txt")).is_ok_and(|bytes| bytes == expected));
}

#[test]
fn self_bound_evidence_stays_false_without_feedback_loop() {
    let lab = Lab::new();
    let root = lab.path().join("root");
    fixture(&root);
    let root_text = root.to_str().unwrap();
    lab.proof(&["manifest", "create", root_text]);
    lab.configure(Some(json!({"root":root})), vec![]);
    lab.ok(&["--on"]);
    until(|| state(&root, "canonical_tree", true));
    assert!(state(&root, "sha_present", true));
    assert!(state(&root, "sha_scope", false));
    assert!(state(&root, "sha_checksum", false));
    sleep(Duration::from_millis(250));
    let stamp = fs::metadata(root.join("state.txt"))
        .unwrap()
        .modified()
        .unwrap();
    sleep(Duration::from_millis(250));
    assert_eq!(
        fs::metadata(root.join("state.txt"))
            .unwrap()
            .modified()
            .unwrap(),
        stamp
    );
    lab.proof(&["manifest", "create", root_text, "--derived", "state.txt"]);
    until(|| state(&root, "sha_scope", true) && state(&root, "sha_checksum", true));
}

#[test]
fn malformed_config_does_not_replace_saved_configuration() {
    let lab = Lab::new();
    let initial = lab.ok(&["--config"]);
    fs::write(lab.path().join("bad.json"), b"{}").unwrap();
    assert_eq!(lab.output(&["--config", "bad.json"]).status.code(), Some(2));
    assert_eq!(lab.ok(&["--config"])["config"], initial["config"]);
    assert_eq!(lab.ok(&["--off"])["running"], false);
    assert_eq!(lab.ok(&["--off"])["running"], false);
}
