use flate2::read::GzDecoder;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

struct Sandbox(TempDir);

impl Sandbox {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn path(&self, path: &str) -> PathBuf {
        self.0.path().join(path)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_devtool"));
        command
            .current_dir(self.0.path())
            .args(args)
            .env("XDG_CONFIG_HOME", self.path("settings"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("HOME", self.path("home"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("DEVTOOL_") {
                command.env_remove(key);
            }
        }
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> Output {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_slice(&self.ok(args).stdout).unwrap()
    }

    fn input(&self, args: &[&str], input: &[u8]) -> Output {
        let mut process = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        process.stdin.take().unwrap().write_all(input).unwrap();
        process.wait_with_output().unwrap()
    }
}

#[test]
fn help_version_and_argument_errors() {
    let sandbox = Sandbox::new();
    let help = sandbox.ok(&["--help"]);
    assert!(String::from_utf8(help.stdout).unwrap().contains("compress"));
    sandbox.ok(&["file", "create", "--help"]);
    sandbox.ok(&["config", "--help"]);
    assert_eq!(
        String::from_utf8(sandbox.ok(&["--version"]).stdout)
            .unwrap()
            .trim(),
        "devtool 0.5.0"
    );
    assert_eq!(
        sandbox
            .run(&["compress", "x", "--level", "10"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        sandbox
            .run(&["file", "create", "x", "--append", "--overwrite"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        sandbox
            .run(&["file", "create", "x", "--stdin", "--content", "x"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn directories_and_parent_overrides() {
    let sandbox = Sandbox::new();
    sandbox.ok(&["mkdir", "one/two", "three", "--parents"]);
    assert!(sandbox.path("one/two").is_dir());
    assert!(sandbox.path("three").is_dir());
    assert!(
        !sandbox
            .run(&["mkdir", "absent/child", "--no-parents"])
            .status
            .success()
    );
    sandbox.ok(&["config", "set", "filesystem.create_parents", "false"]);
    assert!(!sandbox.run(&["mkdir", "four/five"]).status.success());
    sandbox.ok(&["mkdir", "four/five", "--parents"]);
}

#[test]
fn file_content_append_overwrite_and_binary_stdin() {
    let sandbox = Sandbox::new();
    sandbox.ok(&["file", "create", "folder/hello.txt", "--content", "Hallo"]);
    assert!(
        !sandbox
            .run(&["file", "create", "folder/hello.txt"])
            .status
            .success()
    );
    assert_eq!(
        fs::read(sandbox.path("folder/hello.txt")).unwrap(),
        b"Hallo"
    );
    sandbox.ok(&[
        "file",
        "create",
        "folder/hello.txt",
        "--append",
        "--content",
        " wereld",
    ]);
    assert_eq!(
        fs::read(sandbox.path("folder/hello.txt")).unwrap(),
        b"Hallo wereld"
    );
    sandbox.ok(&[
        "file",
        "create",
        "folder/hello.txt",
        "--overwrite",
        "--content",
        "Nieuw",
    ]);
    assert_eq!(
        fs::read(sandbox.path("folder/hello.txt")).unwrap(),
        b"Nieuw"
    );
    let bytes = b"\0\xff\nUTF-8 is optional\0";
    assert!(
        sandbox
            .input(&["file", "create", "binary", "--stdin"], bytes)
            .status
            .success()
    );
    assert_eq!(fs::read(sandbox.path("binary")).unwrap(), bytes);
    sandbox.ok(&["file", "create", "empty"]);
    assert_eq!(fs::metadata(sandbox.path("empty")).unwrap().len(), 0);
}

#[test]
fn configuration_is_dynamic_and_typed() {
    let sandbox = Sandbox::new();
    sandbox.ok(&["config", "init"]);
    assert!(!sandbox.run(&["config", "init"]).status.success());
    sandbox.ok(&["config", "set", "compression.format", "7z"]);
    assert_eq!(
        sandbox.json(&["--json", "config", "get", "compression.format"]),
        "7z"
    );
    sandbox.ok(&["config", "set", "compression.level", "2"]);
    assert_eq!(
        sandbox.json(&["--json", "config", "get", "compression.level"]),
        2
    );
    assert!(
        !sandbox
            .run(&["config", "set", "compression.level", "10"])
            .status
            .success()
    );
    assert!(
        !sandbox
            .run(&["config", "set", "filesystem.create_parents", "yes"])
            .status
            .success()
    );
    assert!(
        !sandbox
            .run(&["config", "set", "unknown", "x"])
            .status
            .success()
    );
    assert_eq!(
        sandbox.json(&["--json", "config", "get", "compression.level"]),
        2
    );
    sandbox.ok(&["config", "validate"]);
    sandbox.ok(&["config", "init", "--force"]);
    assert_eq!(
        sandbox.json(&["--json", "config", "get", "compression.level"]),
        6
    );
}

#[test]
fn effective_settings_report_precedence_and_origins() {
    let sandbox = Sandbox::new();
    sandbox.ok(&["config", "set", "compression.level", "2"]);
    let output = sandbox
        .command(&["--json", "config", "show", "--effective"])
        .env("DEVTOOL_COMPRESSION_LEVEL", "4")
        .env("DEVTOOL_OUTPUT_FORMAT", "text")
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["settings"]["compression"]["level"], 4);
    assert_eq!(
        value["sources"]["compression.level"],
        "env:DEVTOOL_COMPRESSION_LEVEL"
    );
    assert_eq!(value["sources"]["output.format"], "cli");
    assert_eq!(value["sources"]["filesystem.existing_file"], "file");
    assert_eq!(
        sandbox.json(&["--json", "config", "show"])["compression"]["level"],
        2
    );
    fs::write(sandbox.path("source"), b"contents").unwrap();
    let output = sandbox
        .command(&[
            "--json",
            "compress",
            "source",
            "--level",
            "1",
            "-o",
            "source.7z",
            "--format",
            "7z",
        ])
        .env("DEVTOOL_COMPRESSION_LEVEL", "3")
        .env("DEVTOOL_COMPRESSION_FORMAT", "tar.gz")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["level"], 1);
    assert_eq!(value["format"], "7z");
}

#[test]
fn setting_does_not_persist_environment_values() {
    let sandbox = Sandbox::new();
    let output = sandbox
        .command(&["config", "set", "compression.level", "3"])
        .env("DEVTOOL_COMPRESSION_FORMAT", "7z")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        sandbox.json(&["--json", "config", "get", "compression.format"]),
        "tar.gz"
    );
}

#[test]
fn explicit_configuration_and_missing_files() {
    let sandbox = Sandbox::new();
    assert!(
        !sandbox
            .run(&["--config", "custom.toml", "mkdir", "x"])
            .status
            .success()
    );
    sandbox.ok(&["--config", "custom.toml", "config", "init"]);
    sandbox.ok(&[
        "config",
        "set",
        "compression.level",
        "1",
        "--config",
        "custom.toml",
    ]);
    assert_eq!(
        sandbox.json(&[
            "--json",
            "--config",
            "custom.toml",
            "config",
            "get",
            "compression.level"
        ]),
        1
    );
    let output = sandbox
        .command(&["--json", "config", "get", "compression.level"])
        .env("DEVTOOL_CONFIG", sandbox.path("custom.toml"))
        .output()
        .unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&output.stdout).unwrap(), 1);
}

#[test]
fn malformed_configuration_is_rejected_and_recoverable() {
    let sandbox = Sandbox::new();
    for text in [
        "[compression]\nlevel = 10\n",
        "[unknown]\nx = 1\n",
        "schema_version = 2\n",
        "[invalid",
    ] {
        fs::write(sandbox.path("bad.toml"), text).unwrap();
        assert!(
            !sandbox
                .run(&["--config", "bad.toml", "config", "validate"])
                .status
                .success()
        );
        assert!(
            !sandbox
                .run(&["--config", "bad.toml", "mkdir", "x"])
                .status
                .success()
        );
    }
    sandbox.ok(&["--config", "bad.toml", "config", "init", "--force"]);
    sandbox.ok(&["--config", "bad.toml", "config", "validate"]);
}

#[test]
fn configuration_menu_saves_only_on_request() {
    let sandbox = Sandbox::new();
    let output = sandbox.input(&["config", "menu"], b"5\n2\ns\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        sandbox.json(&["--json", "config", "get", "compression.level"]),
        2
    );
    assert!(
        sandbox
            .input(&["config", "menu"], b"5\n3\nq\n")
            .status
            .success()
    );
    assert_eq!(
        sandbox.json(&["--json", "config", "get", "compression.level"]),
        2
    );
    assert!(!sandbox.input(&["config", "menu"], b"").status.success());
}

#[test]
fn schemas_are_json_and_match_commands_and_settings() {
    let sandbox = Sandbox::new();
    let schema = sandbox.json(&["config", "schema"]);
    assert_eq!(schema["properties"]["schema_version"]["maximum"], 1);
    assert_eq!(
        schema["$defs"]["Compression"]["properties"]["level"]["maximum"],
        9
    );
    assert_eq!(schema["additionalProperties"], false);
    let cli = sandbox.json(&["schema"]);
    assert!(
        cli["cli"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command["name"] == "compress")
    );
    sandbox.ok(&["schema", "--output", "schemas/cli.json"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(sandbox.path("schemas/cli.json")).unwrap())
            .unwrap(),
        cli
    );
    sandbox.ok(&["config", "schema", "--output", "schemas/config.json"]);
}

#[test]
fn archive_roundtrips_preserve_contents_and_empty_directories() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(sandbox.path("source/sub/empty")).unwrap();
    fs::write(
        sandbox.path("source/sub/bericht.txt"),
        "Hallo wereld — Rust\n",
    )
    .unwrap();
    fs::write(sandbox.path("source/bytes"), [0, 255, 1, 2, 0]).unwrap();
    for (format, output, destination) in [
        ("tar.gz", "bundle.tar.gz", "unpacked-tar"),
        ("7z", "bundle.7z", "unpacked-7z"),
    ] {
        let report = sandbox.json(&[
            "--json", "compress", "source", "--format", format, "--level", "1", "-o", output,
        ]);
        assert_eq!(report["entries"], 5);
        assert!(report["bytes"].as_u64().unwrap() > 0);
        if format == "tar.gz" {
            tar::Archive::new(GzDecoder::new(
                fs::File::open(sandbox.path(output)).unwrap(),
            ))
            .unpack(sandbox.path(destination))
            .unwrap();
        } else {
            sevenz_rust2::decompress_file(sandbox.path(output), sandbox.path(destination)).unwrap();
        }
        assert_eq!(
            fs::read(sandbox.path(&format!("{destination}/source/sub/bericht.txt"))).unwrap(),
            "Hallo wereld — Rust\n".as_bytes()
        );
        assert_eq!(
            fs::read(sandbox.path(&format!("{destination}/source/bytes"))).unwrap(),
            [0, 255, 1, 2, 0]
        );
        assert!(
            sandbox
                .path(&format!("{destination}/source/sub/empty"))
                .is_dir()
        );
    }
}

#[test]
fn compression_writes_the_selected_output_without_an_overwrite_flag() {
    for format in ["tar.gz", "7z"] {
        let sandbox = Sandbox::new();
        fs::create_dir(sandbox.path("source")).unwrap();
        fs::write(sandbox.path("source/a"), b"original contents").unwrap();
        let output = format!("archive.{format}");
        fs::write(sandbox.path(&output), b"previous output bytes").unwrap();
        let report = sandbox.json(&[
            "--json", "compress", "source", "--format", format, "--level", "1", "-o", &output,
        ]);
        assert_eq!(report["entries"], 2);
        assert_ne!(
            fs::read(sandbox.path(&output)).unwrap(),
            b"previous output bytes"
        );
        match format {
            "tar.gz" => tar::Archive::new(GzDecoder::new(
                fs::File::open(sandbox.path(&output)).unwrap(),
            ))
            .unpack(sandbox.path("restored"))
            .unwrap(),
            _ => sevenz_rust2::decompress_file(sandbox.path(&output), sandbox.path("restored"))
                .unwrap(),
        }
        assert_eq!(
            fs::read(sandbox.path("restored/source/a")).unwrap(),
            b"original contents"
        );
    }
}

#[test]
fn compression_uses_the_complete_source_snapshot() {
    for format in ["tar.gz", "7z"] {
        let sandbox = Sandbox::new();
        fs::create_dir(sandbox.path("source")).unwrap();
        fs::write(sandbox.path("source/a"), b"contents").unwrap();
        let output = format!("source/archive.{format}");
        fs::write(sandbox.path(&output), b"preexisting source file").unwrap();
        let report = sandbox.json(&[
            "--json", "compress", "source", "--format", format, "--level", "1", "-o", &output,
        ]);
        assert_eq!(report["entries"], 3);
        match format {
            "tar.gz" => tar::Archive::new(GzDecoder::new(
                fs::File::open(sandbox.path(&output)).unwrap(),
            ))
            .unpack(sandbox.path("restored"))
            .unwrap(),
            _ => sevenz_rust2::decompress_file(sandbox.path(&output), sandbox.path("restored"))
                .unwrap(),
        }
        assert_eq!(
            fs::read(sandbox.path(&format!("restored/{output}"))).unwrap(),
            b"preexisting source file"
        );
    }
}

#[test]
fn single_files_default_names_and_invalid_sources() {
    let sandbox = Sandbox::new();
    fs::write(sandbox.path("file"), b"sample").unwrap();
    sandbox.ok(&["compress", "file", "--format", "7z", "--level", "0"]);
    assert!(sandbox.path("file.7z").exists());
    sevenz_rust2::decompress_file(sandbox.path("file.7z"), sandbox.path("unpacked")).unwrap();
    assert_eq!(fs::read(sandbox.path("unpacked/file")).unwrap(), b"sample");
    sandbox.ok(&["compress", "file", "--format", "7z", "-o", "file"]);
    sevenz_rust2::decompress_file(sandbox.path("file"), sandbox.path("restored-in-place")).unwrap();
    assert_eq!(
        fs::read(sandbox.path("restored-in-place/file")).unwrap(),
        b"sample"
    );
    assert!(
        !sandbox
            .run(&["compress", "missing", "-o", "missing.tar.gz"])
            .status
            .success()
    );
    assert!(!sandbox.path("missing.tar.gz").exists());
}

#[test]
fn output_preferences_apply_to_later_commands() {
    let sandbox = Sandbox::new();
    sandbox.ok(&["config", "set", "output.format", "json"]);
    assert!(sandbox.json(&["mkdir", "hello"])["directories"].is_array());
    let mut command = sandbox.command(&["mkdir", "color", "--color", "always"]);
    command.env("DEVTOOL_OUTPUT_FORMAT", "text");
    let output = command.output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.starts_with(b"\x1b[32m"));
}

#[cfg(unix)]
#[test]
fn compression_handles_symbolic_file_links_through_the_archive_backends() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("source")).unwrap();
    fs::write(sandbox.path("source/file"), b"contents").unwrap();
    std::os::unix::fs::symlink("file", sandbox.path("source/link")).unwrap();
    sandbox.ok(&["compress", "source", "-o", "source.tar.gz"]);
    tar::Archive::new(GzDecoder::new(
        fs::File::open(sandbox.path("source.tar.gz")).unwrap(),
    ))
    .unpack(sandbox.path("unpacked"))
    .unwrap();
    assert_eq!(
        fs::read_link(sandbox.path("unpacked/source/link")).unwrap(),
        PathBuf::from("file")
    );
    sandbox.ok(&["compress", "source", "--format", "7z", "-o", "source.7z"]);
    sevenz_rust2::decompress_file(sandbox.path("source.7z"), sandbox.path("unpacked-7z")).unwrap();
    assert_eq!(
        fs::read(sandbox.path("unpacked-7z/source/link")).unwrap(),
        b"contents"
    );
}

fn check_root_name(value: &Value, run: u64) {
    let timestamp = value["timestamp"].as_str().unwrap();
    assert_eq!(timestamp.len(), 16);
    assert_eq!(&timestamp[8..9], "T");
    assert_eq!(&timestamp[15..], "Z");
    assert!(
        timestamp[..8]
            .bytes()
            .chain(timestamp[9..15].bytes())
            .all(|byte| byte.is_ascii_digit())
    );
    assert_eq!(value["run"], run);
    assert_eq!(value["name"], format!("{timestamp}_run-nr.{run:03}"));
    assert_eq!(
        PathBuf::from(value["path"].as_str().unwrap())
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
        value["name"].as_str().unwrap()
    );
    assert!(PathBuf::from(value["path"].as_str().unwrap()).is_dir());
}

#[test]
fn main_directory_defaults_to_current_working_directory() {
    let sandbox = Sandbox::new();
    let first = sandbox.json(&["-D", "--json"]);
    check_root_name(&first, 1);
    assert_eq!(
        PathBuf::from(first["path"].as_str().unwrap())
            .parent()
            .unwrap(),
        sandbox.0.path()
    );
    let second = sandbox.json(&["--json", "-D"]);
    check_root_name(&second, 2);
    assert_ne!(first["name"], second["name"]);
}

#[test]
fn main_directory_counter_is_shared_across_parent_paths_and_configs() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("first")).unwrap();
    fs::create_dir(sandbox.path("second")).unwrap();
    let first = sandbox.json(&["-D", "--path", "first", "--json"]);
    check_root_name(&first, 1);
    assert_eq!(
        PathBuf::from(first["path"].as_str().unwrap())
            .parent()
            .unwrap(),
        sandbox.path("first")
    );
    sandbox.ok(&["--config", "other.toml", "config", "init"]);
    let second = sandbox.json(&["--config", "other.toml", "-D", "--path", "second", "--json"]);
    check_root_name(&second, 2);
    assert_eq!(
        PathBuf::from(second["path"].as_str().unwrap())
            .parent()
            .unwrap(),
        sandbox.path("second")
    );
}

#[test]
fn main_directory_continues_035_036_and_does_not_wrap_at_999() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(sandbox.path("state/devtool")).unwrap();
    let counter = sandbox.path("state/devtool/directory-counter.json");
    fs::write(&counter, br#"{"schema_version":1,"last_run":34}"#).unwrap();
    check_root_name(&sandbox.json(&["-D", "--json"]), 35);
    check_root_name(&sandbox.json(&["-D", "--json"]), 36);
    fs::write(&counter, br#"{"schema_version":1,"last_run":999}"#).unwrap();
    check_root_name(&sandbox.json(&["-D", "--json"]), 1000);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&counter).unwrap()).unwrap()["last_run"],
        1000
    );
}

#[test]
fn main_directory_parallel_processes_allocate_unique_run_numbers() {
    let sandbox = Sandbox::new();
    let processes: Vec<_> = (0..12)
        .map(|_| {
            sandbox
                .command(&["-D", "--json"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut runs = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for process in processes {
        let output = process.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let run = value["run"].as_u64().unwrap();
        check_root_name(&value, run);
        runs.push(run);
        assert!(names.insert(value["name"].as_str().unwrap().to_owned()));
    }
    runs.sort();
    assert_eq!(runs, (1..=12).collect::<Vec<_>>());
}

#[test]
fn main_directory_flag_contract_and_parent_failure() {
    let sandbox = Sandbox::new();
    for args in [
        vec!["--path", "x"],
        vec!["-D", "mkdir", "x"],
        vec!["-d"],
        vec!["-D", "--path"],
    ] {
        assert_eq!(sandbox.run(&args).status.code(), Some(2), "{args:?}");
    }
    sandbox.ok(&["config", "set", "filesystem.create_parents", "true"]);
    assert!(
        !sandbox
            .run(&["-D", "--path", "missing/parent"])
            .status
            .success()
    );
    assert!(
        !sandbox
            .path("state/devtool/directory-counter.json")
            .exists()
    );
    check_root_name(&sandbox.json(&["-D", "--json"]), 1);
}

#[test]
fn main_directory_counter_errors_do_not_reset_the_sequence() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(sandbox.path("state/devtool")).unwrap();
    let path = sandbox.path("state/devtool/directory-counter.json");
    for bytes in [
        b"invalid".as_slice(),
        br#"{"schema_version":2,"last_run":35}"#,
        br#"{"schema_version":1,"last_run":18446744073709551615}"#,
    ] {
        fs::write(&path, bytes).unwrap();
        assert!(!sandbox.run(&["-D"]).status.success());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn main_directory_naming_contract_is_exposed_in_cli_schema() {
    let sandbox = Sandbox::new();
    let schema = sandbox.json(&["schema"]);
    assert_eq!(
        schema["directory_mode"]["name_template"],
        "YYYYMMDDTHHMMSSZ_run-nr.NNN"
    );
    assert_eq!(schema["directory_mode"]["naming_configurable"], false);
    assert!(
        schema["cli"]["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["short"] == "D")
    );
    assert!(
        !sandbox
            .run(&["config", "set", "directory.name", "custom"])
            .status
            .success()
    );
}

fn assert_canonical_tree(path: &std::path::Path) {
    let template: Value = serde_json::from_str(devtool::canonical::TEMPLATE_JSON).unwrap();
    assert_eq!(template["directories"].as_array().unwrap().len(), 4);
    assert_eq!(template["files"].as_array().unwrap().len(), 17);
    let root_entries = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(root_entries.iter().filter(|path| path.is_dir()).count(), 4);
    assert_eq!(root_entries.iter().filter(|path| path.is_file()).count(), 5);
    for directory in ["docs", "data", "logs", "output"] {
        assert_eq!(fs::read_dir(path.join(directory)).unwrap().count(), 3);
    }
    for file in template["files"].as_array().unwrap() {
        let filename = file["path"].as_str().unwrap();
        assert_eq!(
            fs::read(path.join(filename)).unwrap(),
            devtool::canonical::FILES
                .iter()
                .find(|entry| entry.path == filename)
                .unwrap()
                .bytes,
            "{filename}"
        );
    }
    assert!(
        fs::read(path.join("document.pdf"))
            .unwrap()
            .starts_with(b"%PDF-1.4\n")
    );
    assert_eq!(
        fs::read(path.join("state.txt")).unwrap(),
        include_bytes!("../templates/files/state.txt")
    );
    assert!(fs::read(path.join("checksums.sha")).unwrap().is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(path.join("data/records.json")).unwrap())
            .unwrap(),
        serde_json::json!([])
    );
}

#[test]
fn canonical_tree_defaults_to_latest_main_directory_across_working_directories() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("first")).unwrap();
    fs::create_dir(sandbox.path("second")).unwrap();
    let first = sandbox.json(&["-D", "--path", "first", "--json"]);
    let latest = sandbox.json(&["-D", "--path", "second", "--json"]);
    fs::create_dir(sandbox.path("elsewhere")).unwrap();
    let output = sandbox
        .command(&["--canonical-tree", "--json"])
        .current_dir(sandbox.path("elsewhere"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["path"], latest["path"]);
    assert_eq!(report["runner"], "embedded-shell");
    assert_eq!(report["directories"].as_array().unwrap().len(), 4);
    assert_eq!(report["files"].as_array().unwrap().len(), 17);
    assert_canonical_tree(std::path::Path::new(latest["path"].as_str().unwrap()));
    assert_eq!(
        fs::read_dir(first["path"].as_str().unwrap())
            .unwrap()
            .count(),
        0
    );
    assert_eq!(fs::read_dir(sandbox.path("elsewhere")).unwrap().count(), 0);
}

#[test]
fn canonical_tree_explicit_target_works_without_a_previous_main_directory() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("chosen tree's $folder")).unwrap();
    let report = sandbox.json(&[
        "--canonical-tree",
        "--path",
        "chosen tree's $folder",
        "--json",
    ]);
    assert_eq!(
        report["path"],
        sandbox.path("chosen tree's $folder").to_str().unwrap()
    );
    assert_canonical_tree(&sandbox.path("chosen tree's $folder"));
    assert!(
        !sandbox
            .path("state/devtool/directory-counter.json")
            .exists()
    );
}

#[test]
fn canonical_tree_executes_fixed_copy_commands_with_embedded_bytes() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("target")).unwrap();
    fs::write(sandbox.path("target/README.md"), b"previous contents").unwrap();
    fs::write(sandbox.path("target/state.txt"), b"previous contents").unwrap();
    let report = sandbox.json(&["--canonical-tree", "--path", "target", "--json"]);
    assert_eq!(report["files"].as_array().unwrap().len(), 17);
    assert!(report.get("existing_files").is_none());
    assert_canonical_tree(&sandbox.path("target"));
}

#[test]
fn canonical_tree_reports_missing_or_removed_latest_main_directory() {
    let sandbox = Sandbox::new();
    let missing = sandbox.run(&["--canonical-tree"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("No latest main directory"));
    let root = sandbox.json(&["-D", "--json"]);
    fs::remove_dir(root["path"].as_str().unwrap()).unwrap();
    let removed = sandbox.run(&["--canonical-tree"]);
    assert!(!removed.status.success());
    assert!(String::from_utf8_lossy(&removed.stderr).contains("shell runner failed"));
    assert!(!sandbox.path("README.md").exists());
}

#[test]
fn canonical_tree_uses_old_counter_without_resetting_the_run_sequence() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(sandbox.path("state/devtool")).unwrap();
    let path = sandbox.path("state/devtool/directory-counter.json");
    fs::write(&path, br#"{"schema_version":1,"last_run":34}"#).unwrap();
    assert!(!sandbox.run(&["--canonical-tree"]).status.success());
    fs::create_dir(sandbox.path("explicit")).unwrap();
    sandbox.ok(&["--canonical-tree", "--path", "explicit"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["last_run"],
        34
    );
    let created = sandbox.json(&["-D", "--json"]);
    check_root_name(&created, 35);
    sandbox.ok(&["--canonical-tree"]);
    assert_canonical_tree(std::path::Path::new(created["path"].as_str().unwrap()));
}

#[test]
fn canonical_tree_explicit_path_and_failed_directory_creation_keep_latest_pointer() {
    let sandbox = Sandbox::new();
    let latest = sandbox.json(&["-D", "--json"]);
    fs::create_dir(sandbox.path("explicit")).unwrap();
    sandbox.ok(&["--canonical-tree", "--path", "explicit"]);
    fs::write(sandbox.path("file"), b"file").unwrap();
    assert!(!sandbox.run(&["-D", "--path", "file"]).status.success());
    let default = sandbox.json(&["--canonical-tree", "--json"]);
    assert_eq!(default["path"], latest["path"]);
    let state: Value = serde_json::from_slice(
        &fs::read(sandbox.path("state/devtool/directory-counter.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["last_root"], latest["path"]);
    assert_eq!(state["last_run"], 1);
}

#[test]
fn canonical_tree_cli_contract_and_schema_include_embedded_template() {
    let sandbox = Sandbox::new();
    for args in [
        vec!["--canonical-tree", "-D"],
        vec!["--canonical-tree", "mkdir", "x"],
    ] {
        assert_eq!(sandbox.run(&args).status.code(), Some(2));
    }
    let schema = sandbox.json(&["schema"]);
    assert_eq!(
        schema["canonical_tree_mode"]["template"]["directories"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        schema["canonical_tree_mode"]["template"]["files"]
            .as_array()
            .unwrap()
            .len(),
        17
    );
    assert!(
        schema["canonical_tree_mode"]
            .get("existing_files")
            .is_none()
    );
}

#[test]
fn canonical_tree_shell_failures_are_reported_without_changing_latest_pointer() {
    let sandbox = Sandbox::new();
    let root = sandbox.json(&["-D", "--json"]);
    fs::create_dir(sandbox.path("target")).unwrap();
    fs::write(sandbox.path("target/docs"), b"a file at a directory path").unwrap();
    let result = sandbox.run(&["--canonical-tree", "--path", "target"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("shell runner failed"));
    let state: Value = serde_json::from_slice(
        &fs::read(sandbox.path("state/devtool/directory-counter.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["last_root"], root["path"]);
}

#[test]
fn canonical_assets_and_direct_commands_match_the_manifest() {
    let manifest: Value = serde_json::from_str(devtool::canonical::TEMPLATE_JSON).unwrap();
    let directories: Vec<&str> = manifest["directories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(directories, devtool::canonical::DIRECTORIES);
    let files = manifest["files"].as_array().unwrap();
    assert_eq!(files.len(), devtool::canonical::FILES.len());
    for (item, embedded) in files.iter().zip(devtool::canonical::FILES) {
        assert_eq!(item["path"], embedded.path);
        let asset = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("templates")
            .join(item["asset"].as_str().unwrap());
        assert_eq!(fs::read(asset).unwrap(), embedded.bytes);
    }
    let runner = devtool::canonical::RUNNER_SH;
    assert!(
        !runner
            .split_whitespace()
            .any(|word| ["if", "elif", "else", "case", "while", "for", "test"].contains(&word))
    );
    assert!(!runner.contains("set -C"));
    assert!(!runner.contains("mkdir -p"));
    assert_eq!(
        runner
            .lines()
            .filter(|line| line.starts_with("mkdir -- "))
            .count(),
        4
    );
    assert_eq!(
        runner
            .lines()
            .filter(|line| line.starts_with("cp -- "))
            .count(),
        17
    );
}

#[test]
fn canonical_tree_archives_restore_all_seventeen_embedded_files() {
    let sandbox = Sandbox::new();
    let root = sandbox.json(&["-D", "--json"]);
    sandbox.ok(&["--canonical-tree"]);
    let source = root["path"].as_str().unwrap();
    let basename = root["name"].as_str().unwrap();
    for format in ["7z", "tar.gz"] {
        let output = format!("project.{format}");
        let report = sandbox.json(&[
            "--json", "compress", source, "--format", format, "--level", "1", "-o", &output,
        ]);
        assert_eq!(report["entries"], 22);
        let destination = format!("restored-{format}");
        match format {
            "7z" => {
                sevenz_rust2::decompress_file(sandbox.path(&output), sandbox.path(&destination))
                    .unwrap()
            }
            _ => tar::Archive::new(GzDecoder::new(
                fs::File::open(sandbox.path(&output)).unwrap(),
            ))
            .unpack(sandbox.path(&destination))
            .unwrap(),
        }
        for file in devtool::canonical::FILES {
            assert_eq!(
                fs::read(sandbox.path(&destination).join(basename).join(file.path)).unwrap(),
                file.bytes,
                "{} {}",
                format,
                file.path
            );
        }
    }
}
